//! Document Link (`textDocument/documentLink`) support.
//!
//! Provides clickable links for `require` / `require_once` / `include` /
//! `include_once` paths that resolve to existing files on disk.

use std::path::{Path, PathBuf};

use bumpalo::Bump;
use mago_span::HasSpan;
use mago_syntax::ast::*;
use tower_lsp::lsp_types::{DocumentLink, Range, Url};

use crate::Backend;
use crate::atom::bytes_to_str;
use crate::util::offset_to_position;

/// A resolved include/require path with its source range in the document.
struct IncludeLink {
    /// Byte offset of the start of the linkable range (the entire
    /// include/require value expression).
    start_offset: usize,
    /// Byte offset of the end of the linkable range.
    end_offset: usize,
    /// The resolved absolute file path on disk.
    resolved_path: PathBuf,
}

impl Backend {
    /// Handle a `textDocument/documentLink` request.
    ///
    /// Parses the file and walks the AST for include/require expressions.
    pub fn handle_document_link(&self, uri: &str, content: &str) -> Option<Vec<DocumentLink>> {
        let file_path = Url::parse(uri).ok().and_then(|u| u.to_file_path().ok());
        let file_dir = file_path.as_deref().and_then(|p| p.parent());

        let arena = Bump::new();
        let file_id = mago_database::file::FileId::new(b"input.php");
        let program = mago_syntax::parser::parse_file_content(&arena, file_id, content.as_bytes());

        let mut links: Vec<DocumentLink> = Vec::new();

        if let Some(dir) = file_dir {
            let mut include_links = Vec::new();
            for stmt in program.statements.iter() {
                collect_include_links_from_statement(stmt, content, dir, &mut include_links);
            }
            for il in include_links {
                if let Ok(target_url) = Url::from_file_path(&il.resolved_path) {
                    let start = offset_to_position(content, il.start_offset);
                    let end = offset_to_position(content, il.end_offset);
                    links.push(DocumentLink {
                        range: Range { start, end },
                        target: Some(target_url),
                        tooltip: Some(il.resolved_path.display().to_string()),
                        data: None,
                    });
                }
            }
        }

        if links.is_empty() { None } else { Some(links) }
    }
}

// ─── AST walking for include/require ────────────────────────────────────────

/// Walk a statement looking for include/require constructs.
fn collect_include_links_from_statement(
    stmt: &Statement<'_>,
    content: &str,
    file_dir: &Path,
    links: &mut Vec<IncludeLink>,
) {
    match stmt {
        Statement::Expression(expr_stmt) => {
            collect_include_links_from_expression(expr_stmt.expression, content, file_dir, links);
        }
        Statement::Namespace(ns) => {
            for s in ns.statements().iter() {
                collect_include_links_from_statement(s, content, file_dir, links);
            }
        }
        Statement::Block(block) => {
            for s in block.statements.iter() {
                collect_include_links_from_statement(s, content, file_dir, links);
            }
        }
        Statement::If(if_stmt) => {
            // Main body statements.
            for s in if_stmt.body.statements() {
                collect_include_links_from_statement(s, content, file_dir, links);
            }
            // Else-if clauses.
            for stmts in if_stmt.body.else_if_statements() {
                for s in stmts {
                    collect_include_links_from_statement(s, content, file_dir, links);
                }
            }
            // Else clause.
            if let Some(else_stmts) = if_stmt.body.else_statements() {
                for s in else_stmts {
                    collect_include_links_from_statement(s, content, file_dir, links);
                }
            }
        }
        Statement::Try(try_stmt) => {
            for s in try_stmt.block.statements.iter() {
                collect_include_links_from_statement(s, content, file_dir, links);
            }
            for catch in try_stmt.catch_clauses.iter() {
                for s in catch.block.statements.iter() {
                    collect_include_links_from_statement(s, content, file_dir, links);
                }
            }
            if let Some(ref finally) = try_stmt.finally_clause {
                for s in finally.block.statements.iter() {
                    collect_include_links_from_statement(s, content, file_dir, links);
                }
            }
        }
        Statement::Foreach(foreach) => {
            for s in foreach.body.statements() {
                collect_include_links_from_statement(s, content, file_dir, links);
            }
        }
        Statement::For(for_stmt) => {
            for s in for_stmt.body.statements() {
                collect_include_links_from_statement(s, content, file_dir, links);
            }
        }
        Statement::While(while_stmt) => {
            for s in while_stmt.body.statements() {
                collect_include_links_from_statement(s, content, file_dir, links);
            }
        }
        Statement::DoWhile(do_while) => {
            collect_include_links_from_statement(do_while.statement, content, file_dir, links);
        }
        Statement::Function(func) => {
            for s in func.body.statements.iter() {
                collect_include_links_from_statement(s, content, file_dir, links);
            }
        }
        Statement::Class(class) => {
            for member in class.members.iter() {
                collect_include_links_from_class_member(member, content, file_dir, links);
            }
        }
        Statement::Interface(iface) => {
            for member in iface.members.iter() {
                collect_include_links_from_class_member(member, content, file_dir, links);
            }
        }
        Statement::Trait(trait_def) => {
            for member in trait_def.members.iter() {
                collect_include_links_from_class_member(member, content, file_dir, links);
            }
        }
        Statement::Enum(enum_def) => {
            for member in enum_def.members.iter() {
                collect_include_links_from_class_member(member, content, file_dir, links);
            }
        }
        Statement::Switch(switch) => {
            let cases = match &switch.body {
                SwitchBody::BraceDelimited(b) => &b.cases,
                SwitchBody::ColonDelimited(b) => &b.cases,
            };
            for case in cases.iter() {
                let stmts = match case {
                    SwitchCase::Expression(c) => &c.statements,
                    SwitchCase::Default(c) => &c.statements,
                };
                for s in stmts.iter() {
                    collect_include_links_from_statement(s, content, file_dir, links);
                }
            }
        }
        Statement::Return(ret) => {
            if let Some(val) = &ret.value {
                collect_include_links_from_expression(val, content, file_dir, links);
            }
        }
        Statement::Echo(echo) => {
            for val in echo.values.iter() {
                collect_include_links_from_expression(val, content, file_dir, links);
            }
        }
        _ => {}
    }
}

/// Walk a class-like member for include/require constructs.
fn collect_include_links_from_class_member(
    member: &ClassLikeMember<'_>,
    content: &str,
    file_dir: &Path,
    links: &mut Vec<IncludeLink>,
) {
    if let ClassLikeMember::Method(method) = member
        && let MethodBody::Concrete(body) = &method.body
    {
        for s in body.statements.iter() {
            collect_include_links_from_statement(s, content, file_dir, links);
        }
    }
}

/// Walk an expression looking for include/require constructs.
fn collect_include_links_from_expression(
    expr: &Expression<'_>,
    content: &str,
    file_dir: &Path,
    links: &mut Vec<IncludeLink>,
) {
    match expr {
        Expression::Construct(construct) => {
            let include_value = match construct {
                construct::Construct::Include(c) => Some(c.value),
                construct::Construct::IncludeOnce(c) => Some(c.value),
                construct::Construct::Require(c) => Some(c.value),
                construct::Construct::RequireOnce(c) => Some(c.value),
                _ => None,
            };
            if let Some(value_expr) = include_value {
                try_resolve_include(value_expr, content, file_dir, links);
            }
        }
        Expression::Parenthesized(p) => {
            collect_include_links_from_expression(p.expression, content, file_dir, links);
        }
        Expression::Assignment(a) => {
            collect_include_links_from_expression(a.rhs, content, file_dir, links);
        }
        _ => {}
    }
}

/// Try to resolve an include/require expression value to a file path.
///
/// Supports:
/// - String literals: `'path/to/file.php'`
/// - `__DIR__ . '/relative.php'`
/// - `dirname(__DIR__) . '/relative.php'`
/// - `dirname(__FILE__) . '/relative.php'`
/// - `dirname(__DIR__, 2) . '/relative.php'` (nested dirname)
fn try_resolve_include(
    expr: &Expression<'_>,
    content: &str,
    file_dir: &Path,
    links: &mut Vec<IncludeLink>,
) {
    let _ = content;
    let span = expr.span();
    let start = span.start.offset as usize;
    let end = span.end.offset as usize;

    // Attempt to statically evaluate the expression to a path string.
    if let Some(path_str) = try_evaluate_path_expr(expr, file_dir) {
        let resolved = normalize_and_resolve(file_dir, &path_str);
        if resolved.exists() {
            links.push(IncludeLink {
                start_offset: start,
                end_offset: end,
                resolved_path: resolved,
            });
        }
    }
}

/// Try to statically evaluate an expression to a path string.
///
/// Returns `None` for dynamic expressions that cannot be resolved.
fn try_evaluate_path_expr(expr: &Expression<'_>, file_dir: &Path) -> Option<String> {
    match expr {
        // Simple string literal: 'file.php' or "file.php"
        Expression::Literal(literal::Literal::String(s)) => {
            // Prefer the parsed value, fall back to raw (which includes quotes).
            let value = match s.value {
                Some(v) => bytes_to_str(v),
                None => strip_quotes(bytes_to_str(s.raw)),
            };
            if value.is_empty() {
                return None;
            }
            // If absolute path, return as-is.
            if value.starts_with('/') {
                return Some(value.to_string());
            }
            // Relative path: resolve relative to file_dir.
            Some(file_dir.join(value).to_string_lossy().to_string())
        }
        // __DIR__ → file_dir
        Expression::MagicConstant(magic_constant::MagicConstant::Directory(_)) => {
            Some(file_dir.to_string_lossy().to_string())
        }
        // __FILE__ → not useful for include path resolution on its own.
        // dirname(__FILE__) is equivalent to __DIR__, which is already
        // handled above. We return None here; the dirname() handler
        // has a special case for __FILE__ arguments.
        Expression::MagicConstant(magic_constant::MagicConstant::File(_)) => None,
        // Binary concatenation: lhs . rhs
        Expression::Binary(binary) if binary.operator.is_concatenation() => {
            let lhs = try_evaluate_path_expr(binary.lhs, file_dir)?;
            let rhs = try_evaluate_path_expr(binary.rhs, file_dir)?;
            Some(format!("{}{}", lhs, rhs))
        }
        // dirname(...) calls
        Expression::Call(call::Call::Function(func_call)) => {
            try_evaluate_dirname_call(func_call, file_dir)
        }
        // Parenthesized expression
        Expression::Parenthesized(p) => try_evaluate_path_expr(p.expression, file_dir),
        _ => None,
    }
}

/// Try to evaluate a `dirname(...)` function call.
///
/// Supports:
/// - `dirname(__DIR__)` → parent of file_dir
/// - `dirname(__FILE__)` → file_dir (dirname of the file)
/// - `dirname(__DIR__, 2)` → grandparent of file_dir
/// - `dirname(dirname(__DIR__))` → grandparent via nesting
fn try_evaluate_dirname_call(call: &call::FunctionCall<'_>, file_dir: &Path) -> Option<String> {
    // Check that the function name is `dirname`.
    match call.function {
        Expression::Identifier(ident) => {
            let name = bytes_to_str(ident.value()).trim_start_matches('\\');
            if !name.eq_ignore_ascii_case("dirname") {
                return None;
            }
        }
        _ => return None,
    };

    let args: Vec<_> = call.argument_list.arguments.iter().collect();
    if args.is_empty() {
        return None;
    }

    // First argument: the path expression.
    // Special-case __FILE__: dirname(__FILE__) == __DIR__ == file_dir,
    // but try_evaluate_path_expr returns None for __FILE__ on its own.
    let first_arg = args[0].value();
    let is_file_magic = matches!(
        first_arg,
        Expression::MagicConstant(magic_constant::MagicConstant::File(_))
    );
    let path_value = if is_file_magic {
        // __FILE__ is /path/to/dir/file.php, so we fabricate a child
        // path under file_dir so that dirname() strips it back to file_dir.
        file_dir.join("__file__").to_string_lossy().to_string()
    } else {
        try_evaluate_path_expr(first_arg, file_dir)?
    };

    // Second argument (optional): levels to go up (default 1).
    let levels = if args.len() >= 2 {
        match args[1].value() {
            Expression::Literal(literal::Literal::Integer(int_lit)) => {
                int_lit.value.unwrap_or(1) as usize
            }
            _ => 1,
        }
    } else {
        1
    };

    // Walk up `levels` directories.
    let mut result = PathBuf::from(&path_value);
    for _ in 0..levels {
        result = result.parent()?.to_path_buf();
    }

    Some(result.to_string_lossy().to_string())
}

/// Normalize a path and resolve it to an absolute path.
fn normalize_and_resolve(file_dir: &Path, path_str: &str) -> PathBuf {
    let path = PathBuf::from(path_str);
    if path.is_absolute() {
        // Canonicalize to resolve `..` and `.` components.
        // If canonicalize fails (path doesn't exist yet), use
        // a manual normalization.
        path.canonicalize()
            .unwrap_or_else(|_| normalize_path(&path))
    } else {
        let abs = file_dir.join(&path);
        abs.canonicalize().unwrap_or_else(|_| normalize_path(&abs))
    }
}

/// Manually normalize a path by resolving `.` and `..` components.
fn normalize_path(path: &Path) -> PathBuf {
    let mut components = Vec::new();
    for component in path.components() {
        match component {
            std::path::Component::ParentDir => {
                components.pop();
            }
            std::path::Component::CurDir => {}
            other => components.push(other),
        }
    }
    components.iter().collect()
}

/// Strip surrounding quotes from a string value.
fn strip_quotes(s: &str) -> &str {
    if s.len() >= 2
        && ((s.starts_with('\'') && s.ends_with('\'')) || (s.starts_with('"') && s.ends_with('"')))
    {
        &s[1..s.len() - 1]
    } else {
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_quotes_single() {
        assert_eq!(strip_quotes("'hello.php'"), "hello.php");
    }

    #[test]
    fn test_strip_quotes_double() {
        assert_eq!(strip_quotes("\"hello.php\""), "hello.php");
    }

    #[test]
    fn test_strip_quotes_no_quotes() {
        assert_eq!(strip_quotes("hello.php"), "hello.php");
    }

    #[test]
    fn test_normalize_path() {
        let p = PathBuf::from("/home/user/project/src/../vendor/file.php");
        let normalized = normalize_path(&p);
        assert_eq!(
            normalized,
            PathBuf::from("/home/user/project/vendor/file.php")
        );
    }

    #[test]
    fn test_normalize_path_current_dir() {
        let p = PathBuf::from("/home/user/./project/./file.php");
        let normalized = normalize_path(&p);
        assert_eq!(normalized, PathBuf::from("/home/user/project/file.php"));
    }
}
