use std::ops::ControlFlow;

use mago_syntax::cst::*;
use tower_lsp::lsp_types::{Location, Position, Url};

use crate::Backend;
use crate::atom::bytes_to_str;
use crate::util::strip_fqn_prefix;

use super::helpers::{extract_string_literal, walk_all_php_expressions};

/// Resolve `env('KEY')` to the matching line in `.env`.
pub(crate) fn resolve_env_definition(
    backend: &Backend,
    content: &str,
    position: Position,
) -> Option<Location> {
    // Quick byte scan: skip the full parse when the file contains no env() calls.
    if !content.contains("env(") {
        return None;
    }
    let cursor_offset = crate::util::position_to_offset(content, position) as usize;
    let key = find_env_usage_at_cursor(content, cursor_offset)?;

    let root = backend.workspace_root.read().clone()?;
    let env_path = root.join(".env");
    if !env_path.exists() {
        return None;
    }

    let env_uri = Url::from_file_path(&env_path).ok()?;
    let env_content = std::fs::read_to_string(&env_path).ok()?;
    let pos = find_env_key_line(&env_content, &key);

    Some(crate::definition::point_location(env_uri, pos))
}

/// Return the env key for the `env('KEY')` call under cursor.
fn find_env_usage_at_cursor(content: &str, cursor_offset: usize) -> Option<String> {
    let mut found: Option<String> = None;
    walk_all_php_expressions(content, &mut |expr| {
        if let Some((key, s, e)) = try_env_call(expr, content)
            && cursor_offset >= s
            && cursor_offset <= e
        {
            found = Some(key.to_string());
            return ControlFlow::Break(());
        }
        ControlFlow::Continue(())
    });
    found
}

fn try_env_call<'c>(expr: &Expression<'_>, content: &'c str) -> Option<(&'c str, usize, usize)> {
    let Expression::Call(Call::Function(fc)) = expr else {
        return None;
    };
    let Expression::Identifier(ident) = fc.function else {
        return None;
    };
    if !strip_fqn_prefix(bytes_to_str(ident.value())).eq_ignore_ascii_case("env") {
        return None;
    }
    let first_arg = fc.argument_list.arguments.iter().next()?.value();
    extract_string_literal(first_arg, content)
}

/// Find the line number of `KEY=` in `.env` content.
fn find_env_key_line(env_content: &str, key: &str) -> Position {
    for (line_idx, line) in env_content.lines().enumerate() {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix(key)
            && rest.trim_start().starts_with('=')
        {
            return Position::new(line_idx as u32, 0);
        }
    }
    Position::new(0, 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env_key_at(content: &str, offset: usize) -> Option<String> {
        find_env_usage_at_cursor(content, offset)
    }

    #[test]
    fn detects_env_call() {
        let php = "<?php\n$v = env('APP_NAME');\n";
        let offset = php.find("APP_NAME").unwrap();
        assert_eq!(env_key_at(php, offset), Some("APP_NAME".into()));
    }

    #[test]
    fn ignores_non_env_string() {
        let php = "<?php\n$v = 'APP_NAME';\n";
        let offset = php.find("APP_NAME").unwrap();
        assert!(env_key_at(php, offset).is_none());
    }

    #[test]
    fn detects_env_with_default() {
        let php = "<?php\nenv('DB_HOST', 'localhost');\n";
        let offset = php.find("DB_HOST").unwrap();
        assert_eq!(env_key_at(php, offset), Some("DB_HOST".into()));
    }

    #[test]
    fn finds_env_key_line() {
        let env = "APP_NAME=Laravel\nDB_HOST=127.0.0.1\n";
        let pos = find_env_key_line(env, "DB_HOST");
        assert_eq!(pos.line, 1);
    }

    #[test]
    fn missing_env_key_returns_line_zero() {
        let env = "APP_NAME=Laravel\n";
        let pos = find_env_key_line(env, "MISSING");
        assert_eq!(pos.line, 0);
    }
}
