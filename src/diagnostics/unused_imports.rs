//! Unused `use` statement dimming.
//!
//! After `update_ast`, compare each `use` declaration against all symbol
//! references in the file.  Any import alias that has zero references
//! gets a diagnostic with `Severity::Hint` and `DiagnosticTag::Unnecessary`,
//! which editors render as dimmed text.
//!
//! We only check class-level `use` imports, including `use function` and
//! `use const`, but not trait `use` inside class bodies.

use std::collections::{HashMap, HashSet};

use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::symbol_map::SymbolKind;

use super::helpers::{ByteRange, compute_use_line_ranges, is_offset_in_ranges};

impl Backend {
    /// Collect unused-import diagnostics for a single file.
    ///
    /// Appends diagnostics to `out`.  The caller publishes them via
    /// `textDocument/publishDiagnostics`.
    pub fn collect_unused_import_diagnostics(
        &self,
        uri: &str,
        content: &str,
        out: &mut Vec<Diagnostic>,
    ) {
        // ── Gather the file's use map (short name → FQN) ────────────────
        let file_use_map: HashMap<String, String> = self.file_use_map(uri);

        if file_use_map.is_empty() {
            return;
        }

        // ── Gather the symbol map ───────────────────────────────────────
        let Some(symbol_map) = self.symbol_map_for(uri) else {
            return;
        };

        // ── Compute byte ranges of `use` statement lines ────────────────
        // We need to exclude ClassReference spans that are part of `use`
        // statements themselves — those are the *import declarations*, not
        // actual usages of the imported name.
        let use_line_ranges = compute_use_line_ranges(content);

        // ── Compute byte spans of whole `use` statements ────────────────
        // Unlike `use_line_ranges` these are not brace-depth filtered (the
        // Blade virtual PHP wraps the template's imports in a function
        // body) and each span covers a wrapped group import in full.
        let use_statement_spans = compute_use_statement_spans(content);

        // ── Also compute byte ranges of class/interface/trait/enum
        //    declaration lines so the content safety-net doesn't count
        //    a class declaration bearing the same short name as a usage. ──
        let decl_line_ranges = compute_declaration_line_ranges(content);

        // ── Collect all referenced short names from the symbol map ──────
        //
        // A `use Foo\Bar;` import is considered "used" if `Bar` appears as:
        //   - A ClassReference name (type hint, new, extends, implements, catch, etc.)
        //   - A MemberAccess subject_text for static access (`Bar::method()`)
        //   - A FunctionCall name matching a `use function` alias
        //   - A ConstantReference name matching a `use const` alias
        //
        // We also check docblock type references, which are already emitted
        // as ClassReference spans by the symbol map extraction.
        let mut referenced_aliases: HashSet<String> = HashSet::new();

        for span in &symbol_map.spans {
            // Skip spans that fall on `use` statement lines — those are
            // the import declarations, not actual usage sites.
            if is_offset_in_ranges(span.start, &use_line_ranges) {
                continue;
            }

            match &span.kind {
                SymbolKind::ClassReference { name, .. } => {
                    // The name may be fully qualified, partially qualified,
                    // or unqualified.  We need to check if the first segment
                    // (or the whole name for unqualified) matches a use alias.
                    let first_segment = extract_first_segment(name);
                    if file_use_map.contains_key(first_segment) {
                        referenced_aliases.insert(first_segment.to_string());
                    }
                }

                SymbolKind::MemberAccess {
                    subject_text,
                    is_static: true,
                    ..
                } => {
                    // Static access: `Foo::bar()` — subject_text is `"Foo"`
                    let trimmed = subject_text.as_str(content).trim();
                    if !trimmed.starts_with('$')
                        && trimmed != "self"
                        && trimmed != "static"
                        && trimmed != "parent"
                    {
                        let first_segment = extract_first_segment(trimmed);
                        if file_use_map.contains_key(first_segment) {
                            referenced_aliases.insert(first_segment.to_string());
                        }
                    }
                }

                SymbolKind::FunctionCall { name, .. } => {
                    // `use function` imports are tracked in the use_map,
                    // so this marks them as referenced (preventing false
                    // "unused import" diagnostics).
                    let first_segment = extract_first_segment(name);
                    if file_use_map.contains_key(first_segment) {
                        referenced_aliases.insert(first_segment.to_string());
                    }
                }

                SymbolKind::ConstantReference { name, .. } => {
                    let first_segment = extract_first_segment(name);
                    if file_use_map.contains_key(first_segment) {
                        referenced_aliases.insert(first_segment.to_string());
                    }
                }

                _ => {}
            }
        }

        // Filter to only aliases the symbol map didn't find.
        let unused_aliases: Vec<&String> = file_use_map
            .keys()
            .filter(|alias| !referenced_aliases.contains(alias.as_str()))
            .collect();

        if unused_aliases.is_empty() {
            return;
        }

        // ── Safety-net: scan raw content for missed references ──────────
        //
        // For each still-unused alias, scan the raw content for the alias
        // appearing as an identifier outside of `use` statement and class
        // declaration lines.  This catches references in attributes,
        // annotations, or other contexts the symbol map might have missed.
        //
        // This avoids false positives for edge cases.
        // ── Find use statement positions in the source ──────────────────
        for alias in &unused_aliases {
            let fqn = match file_use_map.get(alias.as_str()) {
                Some(f) => f,
                None => continue,
            };

            // Double-check: scan content for the alias appearing as an
            // identifier outside of `use` statements and class declarations.
            if alias_is_referenced_in_content(
                content,
                alias,
                fqn,
                &use_line_ranges,
                &decl_line_ranges,
            ) {
                continue;
            }

            if let Some(range) =
                find_use_statement_range(self, uri, content, alias, fqn, &use_statement_spans)
            {
                out.push(Diagnostic {
                    range,
                    severity: Some(DiagnosticSeverity::HINT),
                    code: Some(NumberOrString::String("unused_import".to_string())),
                    code_description: None,
                    source: Some("phpantom".to_string()),
                    message: format!("Unused import '{}'", fqn),
                    related_information: None,
                    tags: Some(vec![DiagnosticTag::UNNECESSARY]),
                    data: None,
                });
            }
        }
    }
}

// ─── Helpers ────────────────────────────────────────────────────────────────

/// Compute the byte ranges of class / interface / trait / enum declaration
/// lines.
///
/// These lines contain the declared name as an identifier, which could
/// collide with an import alias of the same short name.  We exclude them
/// from the content safety-net scan.
fn compute_declaration_line_ranges(content: &str) -> Vec<ByteRange> {
    let mut ranges = Vec::new();
    let mut offset: usize = 0;

    for line in content.split('\n') {
        let trimmed = line.trim_start();
        if (trimmed.starts_with("class ")
            || trimmed.starts_with("interface ")
            || trimmed.starts_with("trait ")
            || trimmed.starts_with("enum ")
            || trimmed.starts_with("abstract class ")
            || trimmed.starts_with("final class ")
            || trimmed.starts_with("readonly class ")
            || trimmed.starts_with("final readonly class ")
            || trimmed.starts_with("readonly final class "))
            // Quick sanity: actual declarations, not comments/strings
            && !trimmed.starts_with("//")
        {
            ranges.push((offset, offset + line.len()));
        }
        offset += line.len() + 1;
    }

    ranges
}

/// Compute the byte spans of the file's `use` statements.
///
/// Each span starts at the `use` keyword (leading indentation excluded)
/// and ends at the end of the statement's last line.  A group import may
/// wrap over several lines, so a `use … {` line without a `;` is joined
/// with the lines up to the one closing it.
fn compute_use_statement_spans(content: &str) -> Vec<ByteRange> {
    let mut spans = Vec::new();
    let mut offset: usize = 0;
    let mut pending_start: Option<usize> = None;

    for line in content.split('\n') {
        let trimmed = line.trim_start();
        // A CRLF file's line still carries its `\r`; the statement ends
        // before it.
        let line_end = offset + line.trim_end_matches('\r').len();

        if let Some(start) = pending_start {
            if trimmed.contains(';') {
                spans.push((start, line_end));
                pending_start = None;
            } else if trimmed.contains('}') {
                // A group import closes on `};`.  A `}` on its own means
                // the `use … {` we latched onto was a trait import with a
                // conflict-resolution block, so stop following it rather
                // than running on to some later statement's semicolon.
                pending_start = None;
            }
        } else if trimmed.starts_with("use ") {
            let start = offset + (line.len() - trimmed.len());
            if trimmed.contains(';') {
                spans.push((start, line_end));
            } else if trimmed.contains('{') {
                // `use App\Models\{` — the members follow on later lines.
                pending_start = Some(start);
            }
        }

        offset += line.len() + 1; // +1 for the '\n' that `split` consumed
    }

    spans
}

/// Extract the first segment of a potentially qualified name.
///
/// - `"Foo"` → `"Foo"`
/// - `"Foo\\Bar"` → `"Foo"`
fn extract_first_segment(name: &str) -> &str {
    name.split('\\').next().unwrap_or(name)
}

/// Check whether an alias name appears as an identifier reference in the
/// file content outside of `use` statements and class declarations.
///
/// This is a simple heuristic safety-net to reduce false positives.  It
/// looks for the alias name preceded and followed by a non-identifier
/// character (word boundary simulation), skipping occurrences on `use`
/// statement lines and class declaration lines.
fn alias_is_referenced_in_content(
    content: &str,
    alias: &str,
    _fqn: &str,
    use_ranges: &[ByteRange],
    decl_ranges: &[ByteRange],
) -> bool {
    let alias_bytes = alias.as_bytes();
    let content_bytes = content.as_bytes();
    let alias_len = alias_bytes.len();

    if alias_len == 0 {
        return false;
    }

    let mut search_from = 0;
    while search_from + alias_len <= content_bytes.len() {
        // Find the next occurrence of the alias string
        let pos = match content[search_from..].find(alias) {
            Some(p) => search_from + p,
            None => break,
        };

        // Check word boundaries.
        //
        // A backslash *after* the alias is a valid boundary: `Assert\Uuid`
        // means the file uses `Assert` as a namespace-alias prefix, which
        // counts as a real usage of the `use … as Assert` import.
        //
        // A backslash *before* the alias is NOT a valid boundary:
        // `Foo\Assert` does not reference a top-level `Assert` alias.
        let before_ok = if pos == 0 {
            true
        } else {
            !is_ident_char(content_bytes[pos - 1])
        };

        let after_ok = if pos + alias_len >= content_bytes.len() {
            true
        } else {
            let next_byte = content_bytes[pos + alias_len];
            next_byte == b'\\' || !is_ident_char(next_byte)
        };

        if before_ok && after_ok {
            // Skip if this occurrence falls on a `use` statement line.
            if is_offset_in_ranges(pos as u32, use_ranges) {
                search_from = pos + alias_len;
                continue;
            }

            // Skip if this occurrence falls on a class/interface/trait/enum
            // declaration line (the declared name matches the alias).
            if is_offset_in_ranges(pos as u32, decl_ranges) {
                search_from = pos + alias_len;
                continue;
            }

            // Skip occurrences inside single-line comments and docblock
            // prose, but allow matches on docblock lines that contain
            // PHPDoc type tags (`@var`, `@param`, `@return`, etc.) since
            // those are legitimate type references.
            let line_start = content[..pos].rfind('\n').map_or(0, |p| p + 1);
            let line_end = content[pos..].find('\n').map_or(content.len(), |p| pos + p);
            let line_prefix = &content[line_start..pos];
            let full_line = &content[line_start..line_end];
            if line_prefix.contains("//") {
                search_from = pos + alias_len;
                continue;
            }
            if (line_prefix.trim_start().starts_with('*')
                || line_prefix.trim_start().starts_with("/**"))
                && !line_contains_phpdoc_type_tag(full_line)
            {
                search_from = pos + alias_len;
                continue;
            }

            // Found a real reference outside excluded lines
            return true;
        }

        search_from = pos + 1;
    }

    false
}

/// PHPDoc tags whose values contain type references that count as real
/// usages of imported classes.
const PHPDOC_TYPE_TAGS: &[&str] = &[
    "@var",
    "@param",
    "@return",
    "@throws",
    "@template",
    "@extends",
    "@implements",
    "@use",
    "@mixin",
    "@method",
    "@property",
    "@property-read",
    "@property-write",
    "@phpstan-type",
    "@psalm-type",
    "@phpstan-import-type",
    "@phpstan-param",
    "@phpstan-return",
    "@phpstan-var",
    "@psalm-param",
    "@psalm-return",
    "@psalm-var",
    "@phpstan-extends",
    "@phpstan-implements",
    "@phpstan-require-extends",
    "@phpstan-require-implements",
    "@phpstan-sealed",
    "@psalm-extends",
    "@psalm-implements",
];

/// Check whether a docblock line contains a PHPDoc tag that carries type
/// references (e.g. `@var list<Subscription>`).
fn line_contains_phpdoc_type_tag(line: &str) -> bool {
    let trimmed = line.trim();
    PHPDOC_TYPE_TAGS.iter().any(|tag| trimmed.contains(tag))
}

/// Check whether a byte is a valid PHP identifier character.
fn is_ident_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'\\' || b > 0x7F
}

/// Strip the leading `use` keyword and any `function` / `const` modifier
/// from a `use` statement, returning the remainder.
fn use_statement_body(stmt: &str) -> Option<&str> {
    let body = stmt.strip_prefix("use ")?.trim_start();
    Some(
        body.strip_prefix("function ")
            .or_else(|| body.strip_prefix("const "))
            .unwrap_or(body)
            .trim_start(),
    )
}

/// Whether joining a group import's prefix and a member name yields `fqn`.
///
/// Compared segment-for-segment without allocating, so `App\Models` +
/// `User` matches `App\Models\User` but not `App\Models\SuperUser`.
fn joined_name_matches(prefix: &str, name: &str, fqn: &str) -> bool {
    let name = name.trim_start_matches('\\');
    if prefix.is_empty() {
        return fqn == name;
    }
    fqn.len() == prefix.len() + 1 + name.len()
        && fqn.starts_with(prefix)
        && fqn.as_bytes()[prefix.len()] == b'\\'
        && fqn.ends_with(name)
}

/// Whether a simple (non-group, non-alias) `use` statement imports exactly
/// `fqn`.
///
/// Extracts the imported name from the statement (handling `use function` /
/// `use const`) and compares it to `fqn` segment-for-segment, ignoring a
/// leading namespace separator on either side. This avoids matching
/// `use App\FooBar;` when looking for `use App\Foo;`.
fn simple_use_imports_exact(stmt: &str, fqn: &str) -> bool {
    let Some(body) = use_statement_body(stmt) else {
        return false;
    };
    let name = match body.split(';').next() {
        Some(n) => n.trim(),
        None => return false,
    };
    // A simple import has no alias clause; reject if one is present so the
    // dedicated alias path handles it.
    if name.contains(" as ") {
        return false;
    }
    name.trim_start_matches('\\') == fqn.trim_start_matches('\\')
}

/// Trim whitespace and comments from both ends of a group import member,
/// returning the span of what is left within `item`.
///
/// A wrapped group may carry comments between its members, as in
/// `Nonexistent, // could be namespace`, and the comment belongs to
/// whichever member `split(',')` happened to attach it to.
fn member_content_span(item: &str) -> (usize, usize) {
    let mut start = 0;

    loop {
        let rest = &item[start..];
        start += rest.len() - rest.trim_start().len();

        let rest = &item[start..];
        let skipped = if rest.starts_with("//") || rest.starts_with('#') {
            rest.find('\n').map(|i| i + 1)
        } else if rest.starts_with("/*") {
            rest.find("*/").map(|i| i + 2)
        } else {
            break;
        };

        match skipped {
            Some(n) => start += n,
            // An unterminated comment swallows the rest of the member.
            None => return (item.len(), item.len()),
        }
    }

    let tail = &item[start..];
    let content_len = ["//", "#", "/*"]
        .iter()
        .filter_map(|opener| tail.find(opener))
        .min()
        .unwrap_or(tail.len());
    (start, start + tail[..content_len].trim_end().len())
}

/// Locate the member of a group import (`use Foo\{Bar, Baz as B};`) that
/// imports `fqn` under `alias`.
///
/// Returns the member's byte span within `decl` (covering any `as` clause)
/// along with the number of members in the group.  `decl` may span several
/// lines, since PHP allows the group body to be wrapped.
fn find_group_member(decl: &str, fqn: &str, alias: &str) -> Option<(usize, usize, usize)> {
    let brace_open = decl.find('{')?;
    let brace_close = decl.rfind('}')?;
    if brace_close < brace_open {
        return None;
    }

    let body = use_statement_body(decl)?;
    let body_start = decl.len() - body.len();
    // Everything between the `use` keyword and the `{` is the shared
    // prefix: `use App\Models\{` → `App\Models`.
    let prefix = decl
        .get(body_start..brace_open)?
        .trim()
        .trim_end_matches('\\')
        .trim_start_matches('\\');
    let fqn = fqn.trim_start_matches('\\');

    let mut member_count = 0;
    let mut found = None;
    let mut offset = brace_open + 1;

    for item in decl[brace_open + 1..brace_close].split(',') {
        let item_start = offset;
        offset += item.len() + 1; // +1 for the comma `split` consumed

        let (content_start, content_end) = member_content_span(item);
        let entry = &item[content_start..content_end];
        if entry.is_empty() {
            // A trailing comma before the closing brace is legal PHP, and
            // so is a comment sitting on a line of its own.
            continue;
        }
        member_count += 1;

        if found.is_some() {
            continue;
        }

        // A mixed group spells the modifier per member:
        // `use Foo\{function bar, const BAZ, Qux};`
        let entry = entry
            .strip_prefix("function ")
            .or_else(|| entry.strip_prefix("const "))
            .unwrap_or(entry)
            .trim_start();
        let (name, member_alias) = match entry.split_once(" as ") {
            Some((n, a)) => (n.trim(), Some(a.trim())),
            None => (entry, None),
        };
        let short_name = name.rsplit('\\').next().unwrap_or(name);

        if joined_name_matches(prefix, name, fqn) && member_alias.unwrap_or(short_name) == alias {
            found = Some((item_start + content_start, item_start + content_end));
        }
    }

    let (start, end) = found?;
    Some((start, end, member_count))
}

/// Find the source range of the `use` statement that imports a given FQN
/// (or alias).
///
/// `use_ranges` are the byte spans produced by
/// [`compute_use_statement_spans`], each covering a whole `use` statement
/// even when it wraps over several lines.
///
/// For group imports (`use Foo\{Bar, Baz}`), if only one member is unused,
/// we highlight just the unused member name within the group.  If the entire
/// group is unused, we highlight the whole statement.
fn find_use_statement_range(
    backend: &Backend,
    uri: &str,
    content: &str,
    alias: &str,
    fqn: &str,
    use_ranges: &[ByteRange],
) -> Option<Range> {
    // The FQN's last segment or the alias — what appears in the `use` line.
    let short_name = fqn.rsplit('\\').next().unwrap_or(fqn);
    let has_alias = short_name != alias;

    for &(stmt_start, stmt_end) in use_ranges {
        let Some(stmt) = content.get(stmt_start..stmt_end) else {
            continue;
        };

        // Match against the declaration only, so a trailing comment can't
        // be mistaken for a group body.
        let decl = stmt.split(';').next().unwrap_or(stmt);

        if decl.contains('{') {
            let Some((member_start, member_end, member_count)) =
                find_group_member(decl, fqn, alias)
            else {
                continue;
            };

            // A one-member group has nothing left to keep, so highlight the
            // whole statement the way a plain `use Foo\Bar;` line would be.
            if member_count > 1 {
                return backend.offset_range_to_lsp_range(
                    uri,
                    content,
                    stmt_start + member_start,
                    stmt_start + member_end,
                );
            }
        } else {
            let is_match = if has_alias {
                // `use Foo\Bar as Alias;`
                decl.contains(fqn) && decl.contains(&format!("as {}", alias))
            } else {
                // Simple: `use Foo\Bar;`. Compare the imported name exactly
                // rather than by substring so `use App\Foo;` is not matched
                // by the `use App\FooBar;` line that shares its prefix.
                simple_use_imports_exact(decl, fqn)
            };

            if !is_match {
                continue;
            }
        }

        return backend.offset_range_to_lsp_range(uri, content, stmt_start, stmt_end);
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: no use-statement or declaration ranges to exclude.
    fn referenced(content: &str, alias: &str) -> bool {
        alias_is_referenced_in_content(content, alias, "", &[], &[])
    }

    #[test]
    fn backslash_after_alias_counts_as_reference() {
        // `Assert\Uuid` uses the `Assert` alias as a namespace prefix.
        let content = r#"<?php
use Symfony\Component\Validator\Constraints as Assert;

class Dto {
    public function __construct(
        #[Assert\Uuid(message: 'bad')]
        public string $id,
    ) {}
}
"#;
        let use_ranges = compute_use_line_ranges(content);
        assert!(
            alias_is_referenced_in_content(content, "Assert", "", &use_ranges, &[]),
            "Assert\\Uuid should count as a usage of the Assert alias"
        );
    }

    #[test]
    fn backslash_before_alias_does_not_count() {
        // `Foo\Assert` does NOT reference a top-level `Assert` alias.
        assert!(!referenced(r#"Foo\Assert"#, "Assert"));
    }

    #[test]
    fn standalone_alias_still_detected() {
        assert!(referenced("new Assert();", "Assert"));
    }

    #[test]
    fn alias_inside_longer_word_not_detected() {
        // `Assertion` contains `Assert` but is a different identifier.
        assert!(!referenced("new Assertion();", "Assert"));
    }

    #[test]
    fn alias_with_static_access_through_namespace() {
        // `Assert\Uuid::V7_MONOTONIC` — the alias `Assert` is used.
        assert!(referenced("Assert\\Uuid::V7_MONOTONIC", "Assert"));
    }

    #[test]
    fn alias_on_use_line_not_counted() {
        let content = "use Foo\\Bar as Assert;\n";
        let use_ranges = compute_use_line_ranges(content);
        assert!(
            !alias_is_referenced_in_content(content, "Assert", "", &use_ranges, &[]),
            "Alias on a use-statement line should not count as a reference"
        );
    }

    #[test]
    fn alias_in_comment_not_counted() {
        assert!(!referenced("// Assert is great\n", "Assert"));
    }

    #[test]
    fn alias_in_docblock_prose_not_counted() {
        assert!(!referenced(" * Assert something here\n", "Assert"));
    }

    #[test]
    fn alias_in_docblock_type_tag_counted() {
        assert!(referenced(" * @param Assert $x\n", "Assert"));
    }

    // ── Group import member lookup ──────────────────────────────────

    #[test]
    fn finds_member_in_single_line_group() {
        let decl = "use App\\Models\\{User, Post}";
        let (start, end, count) = find_group_member(decl, "App\\Models\\Post", "Post").unwrap();
        assert_eq!(&decl[start..end], "Post");
        assert_eq!(count, 2);
    }

    #[test]
    fn finds_member_in_multiline_group() {
        let decl = "use App\\Models\\{\n    User,\n    Post,\n}";
        let (start, end, count) = find_group_member(decl, "App\\Models\\User", "User").unwrap();
        assert_eq!(&decl[start..end], "User");
        assert_eq!(count, 2, "a trailing comma does not add a member");
    }

    #[test]
    fn group_member_span_covers_alias_clause() {
        let decl = "use App\\Models\\{User, Post as BlogPost}";
        let (start, end, _) = find_group_member(decl, "App\\Models\\Post", "BlogPost").unwrap();
        assert_eq!(&decl[start..end], "Post as BlogPost");
    }

    #[test]
    fn group_member_matches_nested_name() {
        let decl = "use App\\{Models\\User, Post}";
        let (start, end, _) = find_group_member(decl, "App\\Models\\User", "User").unwrap();
        assert_eq!(&decl[start..end], "Models\\User");
    }

    #[test]
    fn group_member_matches_per_member_modifier() {
        let decl = "use App\\{function helper, const LIMIT}";
        let (start, end, _) = find_group_member(decl, "App\\LIMIT", "LIMIT").unwrap();
        assert_eq!(&decl[start..end], "const LIMIT");
    }

    #[test]
    fn group_member_does_not_match_prefix_sharing_name() {
        // `App\Models\SuperUser` must not be matched when looking for
        // `App\Models\User`.
        let decl = "use App\\Models\\{SuperUser}";
        assert!(find_group_member(decl, "App\\Models\\User", "User").is_none());
    }

    #[test]
    fn group_member_requires_matching_alias() {
        let decl = "use App\\Models\\{Post as BlogPost}";
        assert!(find_group_member(decl, "App\\Models\\Post", "Post").is_none());
    }

    #[test]
    fn group_member_after_a_comment_is_still_found() {
        let decl = "use Uses\\{\n\tBar,\n\tNonexistent, // could be namespace\n\tfunction Foo as fooAgain,\n\tconst MY_CONSTANT\n}";
        let (start, end, count) = find_group_member(decl, "Uses\\Foo", "fooAgain").unwrap();
        assert_eq!(&decl[start..end], "function Foo as fooAgain");
        assert_eq!(count, 4);
    }

    #[test]
    fn group_member_trailing_comment_is_not_part_of_the_name() {
        let decl = "use Uses\\{\n\tBar, // keep\n\tBaz\n}";
        let (start, end, _) = find_group_member(decl, "Uses\\Bar", "Bar").unwrap();
        assert_eq!(&decl[start..end], "Bar");
    }

    // ── Statement spans ─────────────────────────────────────────────

    fn spans(content: &str) -> Vec<&str> {
        compute_use_statement_spans(content)
            .into_iter()
            .map(|(s, e)| &content[s..e])
            .collect()
    }

    #[test]
    fn a_wrapped_group_import_is_one_span() {
        let content = "<?php\nuse App\\Models\\{\n    User,\n    Post,\n};\n\nclass Foo {}\n";
        assert_eq!(
            spans(content),
            ["use App\\Models\\{\n    User,\n    Post,\n};"]
        );
    }

    #[test]
    fn an_indented_use_span_starts_at_the_keyword() {
        let content = "<?php\nnamespace App {\n    use App\\Models\\User;\n}\n";
        assert_eq!(spans(content), ["use App\\Models\\User;"]);
    }

    #[test]
    fn a_trait_use_with_a_conflict_block_does_not_swallow_later_statements() {
        // `use A, B {` opens a conflict-resolution block, not a group
        // import; the scan must let go of it at the closing brace.
        let content = "<?php\nclass Foo {\n    use A, B {\n    }\n}\nuse App\\Models\\User;\n";
        assert_eq!(spans(content), ["use App\\Models\\User;"]);
    }

    #[test]
    fn simple_import_does_not_match_longer_sibling() {
        assert!(simple_use_imports_exact("use App\\Foo;", "App\\Foo"));
        assert!(!simple_use_imports_exact("use App\\FooBar;", "App\\Foo"));
    }
}
