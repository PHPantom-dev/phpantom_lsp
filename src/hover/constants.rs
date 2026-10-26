//! Global constant hover.
//!
//! The value comes from the constant lookup chain in
//! `resolution/constants.rs`.

use tower_lsp_server::gen_lsp_types::Hover;

use crate::Backend;

use super::HoverSite;
use super::formatting::make_hover;

impl Backend {
    /// Hover for a global constant reference: its value when known.
    pub(super) fn hover_constant_reference(
        &self,
        site: &HoverSite,
        symbol_start: u32,
        name: &str,
    ) -> Option<Hover> {
        let ctx = site.ctx;
        // The name is resolved against the file the same way the
        // type engine resolves it, so a namespaced constant is
        // found through whichever spelling the reference used.
        let lookup = self.resolve_constant_name_at(
            name,
            ctx.resolved_names.as_deref(),
            symbol_start,
            &ctx.use_map,
            &ctx.symbol_imports.constants,
            &ctx.namespace,
        );

        // `lookup` is `Some(Some(val))` when the constant
        // exists with a known value, `Some(None)` when it
        // exists but the value is unknown, and `None` when
        // the constant was not found at all.
        match lookup {
            Some(Some(val)) => Some(make_hover(format!(
                "```php\n<?php\nconst {} = {};\n```",
                name, val
            ))),
            Some(None) => Some(make_hover(format!("```php\n<?php\nconst {};\n```", name))),
            None => None,
        }
    }
}

/// Extract the value of a constant from PHP source text.
///
/// Scans for patterns like:
/// - `define('NAME', value)` or `define("NAME", value)`
/// - `const NAME = value;`
///
/// Returns `Some(value_string)` when found, `None` when the constant
/// definition could not be located or the value could not be extracted.
///
/// **Note:** Production code should use `update_ast` to parse constants
/// through the AST pipeline (which populates `global_defines`).  This
/// function exists only for unit tests.
#[cfg(test)]
pub(super) fn extract_constant_value_from_source(name: &str, source: &str) -> Option<String> {
    // Try `define('NAME', value)` pattern.
    for quote in &["'", "\""] {
        let needle = format!("define({quote}{name}{quote}");
        if let Some(pos) = source.find(&needle) {
            // Extract only the second argument.  Stop at the first
            // unquoted comma (third argument) or closing paren,
            // whichever comes first.
            let after = &source[pos + needle.len()..];
            if let Some(comma) = after.find(',') {
                let value_start = &after[comma + 1..];
                let trimmed = value_start.trim_start();
                // Find where the second argument ends: either an
                // unquoted comma (start of optional third arg) or
                // the closing paren.
                let end =
                    find_unquoted_comma(trimmed).or_else(|| find_balanced_close_paren(trimmed));
                if let Some(end) = end {
                    let val = trimmed[..end].trim();
                    if !val.is_empty() {
                        // Empty string literals are placeholders for
                        // runtime-defined values — show the type instead.
                        if val == "''" || val == "\"\"" {
                            return Some("string".to_string());
                        }
                        return Some(val.to_string());
                    }
                }
            }
        }
    }

    // Try `const NAME = value;` pattern.
    let const_needle = format!("const {name}");
    for (i, _) in source.match_indices(&const_needle) {
        let after = &source[i + const_needle.len()..];
        let trimmed = after.trim_start();
        if let Some(rest) = trimmed.strip_prefix('=') {
            let value_part = rest.trim_start();
            if let Some(semi) = value_part.find(';') {
                let val = value_part[..semi].trim();
                if !val.is_empty() {
                    return Some(val.to_string());
                }
            }
        }
    }

    None
}

/// Find the position of the first unquoted comma in `s`.
///
/// Skips over single- and double-quoted string literals so that
/// commas inside string values are not mistaken for argument
/// separators.
#[cfg(test)]
fn find_unquoted_comma(s: &str) -> Option<usize> {
    let mut in_single = false;
    let mut in_double = false;
    let mut prev = b'\0';

    for (i, &b) in s.as_bytes().iter().enumerate() {
        match b {
            b'\'' if !in_double && prev != b'\\' => in_single = !in_single,
            b'"' if !in_single && prev != b'\\' => in_double = !in_double,
            b',' if !in_single && !in_double => return Some(i),
            _ => {}
        }
        prev = b;
    }
    None
}

/// Find the position of the closing `)` that matches an implicit
/// opening paren, handling one level of nesting and string literals.
#[cfg(test)]
fn find_balanced_close_paren(s: &str) -> Option<usize> {
    let mut depth = 0u32;
    let mut in_single = false;
    let mut in_double = false;
    let mut prev = b'\0';

    for (i, &b) in s.as_bytes().iter().enumerate() {
        match b {
            b'\'' if !in_double && prev != b'\\' => in_single = !in_single,
            b'"' if !in_single && prev != b'\\' => in_double = !in_double,
            b'(' if !in_single && !in_double => depth += 1,
            b')' if !in_single && !in_double => {
                if depth == 0 {
                    return Some(i);
                }
                depth -= 1;
            }
            _ => {}
        }
        prev = b;
    }
    None
}
