use mago_allocator::LocalArena;
use mago_database::file::FileId;
use mago_syntax::cst::*;
use tower_lsp::lsp_types::Location;

use crate::Backend;
use crate::atom::bytes_to_str;
use crate::php_type::PhpType;

impl Backend {
    /// The type a translation key resolves to, so that
    /// `__('messages.welcome')` / `trans(...)` / `Lang::get(...)` calls
    /// don't carry the full `string|array|null` union of the framework's
    /// declared return type into an argument a literal key settles.
    ///
    /// A leaf entry is the line itself; a group hands back the nested array
    /// of lines beneath it.  A key the indexed translations do not cover
    /// falls back to [`unresolved_trans_type`].
    pub(crate) fn resolve_trans_type(&self, key: &str) -> Option<PhpType> {
        match self.cached_translations().entries.get(key) {
            Some(entries) if entries.iter().any(|entry| entry.is_group) => Some(trans_group_type()),
            Some(_) => Some(PhpType::string()),
            None => Some(unresolved_trans_type()),
        }
    }
}

/// The type a translation group resolves to: the lines nested beneath it,
/// keyed by their own names.  The values are a mix of lines and further
/// groups, which is as far as a key alone settles the shape.
fn trans_group_type() -> PhpType {
    PhpType::generic_array(PhpType::string(), PhpType::mixed())
}

/// The type a translation call hands back when its key cannot be read: one
/// built at runtime, or one naming lines that are not in the workspace.
///
/// `null` is not among the branches.  `__()` and `trans()` return it only
/// for the keyless form, and every call that names a key at all gets a
/// string back even when the translation is missing (Laravel echoes the key
/// itself).  Which of the two remaining branches applies depends on a key
/// PHPantom cannot see, so the union is benevolent: a call site that passes
/// it on is accepted rather than reported against every branch.
pub(crate) fn unresolved_trans_type() -> PhpType {
    PhpType::benevolent(PhpType::union(vec![PhpType::string(), trans_group_type()]))
}

/// Resolve `__('file.key')` / `trans('file.key')` / `Lang::get('file.key')` to the
/// matching keys inside all matching `lang/{locale}/file.php` translation files,
/// or inside `lang/{locale}.json` JSON translation files.
///
/// For PHP files the key format is `file_stem.nested.key` (first segment = file,
/// rest = array path).  For JSON files the key is looked up directly as a
/// top-level object key (Laravel's JSON translations are flat).
///
/// Whole PHP groups resolve to the start of their file.
pub(crate) fn resolve_trans_definitions(backend: &Backend, key: &str) -> Vec<Location> {
    backend.translation_definitions(key)
}

// ─── Declaration extractor (mirrors config_keys logic) ───────────────────────

#[derive(Debug)]
pub(crate) struct TransKeyMatch {
    pub key: String,
    pub start: usize,
    /// Byte offset immediately after the key's source text, before its quote.
    pub end: usize,
    /// Whether the key's value is itself a nested array (a translation
    /// group) rather than a scalar string entry.
    pub is_group: bool,
    /// The line itself, for a scalar entry written as a string literal.
    pub value: Option<String>,
}

pub(crate) fn collect_trans_declarations(content: &str, file_stem: &str) -> Vec<TransKeyMatch> {
    let arena = LocalArena::new();
    let file_id = FileId::new(b"input.php");
    let program = mago_syntax::parser::parse_file_content(&arena, file_id, content.as_bytes());
    let mut out = Vec::new();

    let mut returned_var_name: Option<String> = None;
    let mut return_expr: Option<&Expression<'_>> = None;

    for stmt in program.statements.iter() {
        if let Statement::Return(ret) = stmt {
            if let Some(val) = ret.value {
                match val {
                    Expression::Variable(Variable::Direct(dv)) => {
                        returned_var_name = Some(bytes_to_str(dv.name).to_string());
                    }
                    _ => {
                        return_expr = Some(val);
                    }
                }
            }
            break;
        }
    }

    if let Some(expr) = return_expr {
        collect_expr(expr, content, file_stem, &[], &mut out);
    } else if let Some(var_name) = returned_var_name {
        for stmt in program.statements.iter() {
            if let Statement::Expression(expr_stmt) = stmt
                && let Expression::Assignment(assign) = expr_stmt.expression
                && let Expression::Variable(Variable::Direct(dv)) = assign.lhs
                && dv.name == var_name.as_bytes()
            {
                collect_expr(assign.rhs, content, file_stem, &[], &mut out);
            }
        }
    }

    out
}

fn collect_expr<'a>(
    expr: &'a Expression<'a>,
    content: &str,
    prefix: &str,
    path: &[String],
    out: &mut Vec<TransKeyMatch>,
) {
    match expr {
        Expression::Array(arr) => {
            collect_array(arr.elements.iter(), content, prefix, path, out);
        }
        Expression::LegacyArray(arr) => {
            collect_array(arr.elements.iter(), content, prefix, path, out);
        }
        Expression::Parenthesized(p) => {
            collect_expr(p.expression, content, prefix, path, out);
        }
        Expression::Call(Call::Function(fc)) => {
            if let Expression::Identifier(ident) = fc.function
                && ident.value().eq_ignore_ascii_case(b"array_merge")
            {
                for arg in fc.argument_list.arguments.iter() {
                    let arg_expr = match arg {
                        Argument::Positional(pos) => pos.value,
                        Argument::Named(named) => named.value,
                    };
                    collect_expr(arg_expr, content, prefix, path, out);
                }
            }
        }
        _ => {}
    }
}

fn collect_array<'a>(
    elements: impl Iterator<Item = &'a ArrayElement<'a>>,
    content: &str,
    prefix: &str,
    path: &[String],
    out: &mut Vec<TransKeyMatch>,
) {
    for element in elements {
        let ArrayElement::KeyValue(kv) = element else {
            continue;
        };
        let Some((key_text, key_start, key_end)) =
            super::helpers::extract_string_literal(kv.key, content)
        else {
            continue;
        };

        let key_text = literal_value(kv.key).unwrap_or(key_text);
        let mut full_path = path.to_vec();
        full_path.push(key_text.to_string());
        let dot_key = format!("{prefix}.{}", full_path.join("."));
        out.push(TransKeyMatch {
            key: dot_key,
            start: key_start,
            end: key_end,
            is_group: value_is_group(kv.value),
            value: literal_value(kv.value).map(str::to_string),
        });

        collect_expr(kv.value, content, prefix, &full_path, out);
    }
}

fn literal_value<'a>(expression: &'a Expression<'_>) -> Option<&'a str> {
    match expression {
        Expression::Literal(Literal::String(string)) => string.value.map(bytes_to_str),
        _ => None,
    }
}

/// Whether a translation entry's value expression is a nested array
/// (a translation group) rather than a scalar string entry.  Mirrors the
/// shapes [`collect_expr`] recurses into, so a group is recognized exactly
/// when there is more beneath it to flatten.
fn value_is_group(expr: &Expression<'_>) -> bool {
    match expr {
        Expression::Array(_) | Expression::LegacyArray(_) => true,
        Expression::Parenthesized(p) => value_is_group(p.expression),
        Expression::Call(Call::Function(fc)) => {
            matches!(fc.function, Expression::Identifier(ident) if ident.value().eq_ignore_ascii_case(b"array_merge"))
        }
        _ => false,
    }
}
