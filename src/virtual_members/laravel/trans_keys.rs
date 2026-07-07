use bumpalo::Bump;
use mago_database::file::FileId;
use mago_syntax::ast::*;
use tower_lsp::lsp_types::{Location, Position, Url};

use crate::Backend;
use crate::atom::bytes_to_str;

/// Resolve `__('file.key')` / `trans('file.key')` / `Lang::get('file.key')` to the
/// matching keys inside all matching `lang/{locale}/file.php` translation files.
///
/// The key format is `file_stem.nested.key` (first segment = file, rest = array path).
/// Falls back to the top of the file when the exact key cannot be located.
pub(crate) fn resolve_trans_definitions(backend: &Backend, key: &str) -> Vec<Location> {
    let Some(file_stem) = key.split('.').next() else {
        return Vec::new();
    };

    let snapshot = backend.user_file_symbol_maps();
    let mut results = Vec::new();
    let target_suffix = format!("/{file_stem}.php");

    for (file_uri, _) in snapshot {
        // Check if the file is in a lang directory and matches the stem.
        if (file_uri.contains("/lang/") || file_uri.contains("/resources/lang/"))
            && file_uri.ends_with(&target_suffix)
        {
            let Ok(uri) = Url::parse(&file_uri) else {
                continue;
            };
            let Some(content) = backend.get_file_content(&file_uri) else {
                continue;
            };

            let declarations = collect_trans_declarations(&content, file_stem);
            if let Some(decl) = declarations.into_iter().find(|d| d.key == key) {
                let pos = crate::util::offset_to_position(&content, decl.start);
                results.push(crate::definition::point_location(uri, pos));
                continue;
            }

            // File matched by stem but key not found — point to top of file.
            results.push(crate::definition::point_location(uri, Position::new(0, 0)));
        }
    }
    results
}

// ─── Declaration extractor (mirrors config_keys logic) ───────────────────────

#[derive(Debug)]
struct TransKeyMatch {
    key: String,
    start: usize,
}

fn collect_trans_declarations(content: &str, file_stem: &str) -> Vec<TransKeyMatch> {
    let arena = Bump::new();
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
        let Some((key_text, key_start, _)) =
            super::helpers::extract_string_literal(kv.key, content)
        else {
            continue;
        };

        let mut full_path = path.to_vec();
        full_path.push(key_text.to_string());
        let dot_key = format!("{prefix}.{}", full_path.join("."));
        out.push(TransKeyMatch {
            key: dot_key,
            start: key_start,
        });

        collect_expr(kv.value, content, prefix, &full_path, out);
    }
}
