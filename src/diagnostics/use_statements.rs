//! How a file's `use` imports are found and picked apart.
//!
//! One scanner, several questions.  [`scan_use_statements`] walks the file
//! once; the wrappers here present its result in the shapes the callers ask
//! for: the diagnostics suppress a class-name report on an import line,
//! `completion::use_edit::analyze_use_block` places a *new* import, and
//! `code_actions::cursor_on_use_import_line` tells whether the cursor rests
//! on one.

use super::helpers::ByteRange;

/// One `use` import statement.
pub(crate) struct UseStatementScan {
    /// Byte offset of the start of the statement's first line, leading
    /// indentation included.
    pub(crate) line_start: usize,
    /// Byte offset of the `use` keyword itself.
    pub(crate) keyword_start: usize,
    /// Byte offset of the end of the statement's last line, excluding a
    /// CRLF file's `\r`.
    pub(crate) end: usize,
    /// Whether the statement sits at import depth: brace depth 0, or depth
    /// 1 inside a `namespace Foo { … }` block.  A trait `use` in a class
    /// body is deeper, and so is the `@php use …;` a Blade template writes,
    /// since the virtual PHP inlines an island inside the wrapper function.
    pub(crate) top_level: bool,
}

/// Scan `content` for `use` imports.
///
/// A statement may wrap over several lines: a group import (`use Foo\{`
/// through its closing `};`), or a plain import split across lines without
/// braces.  Either way it is followed until its terminating `;`.
pub(crate) fn scan_use_statements(content: &str) -> Vec<UseStatementScan> {
    let mut statements = Vec::new();
    let mut offset: usize = 0;
    // Track brace depth so we can distinguish namespace-level `use`
    // imports (depth 0, or depth 1 inside `namespace Foo { … }`) from
    // trait `use` statements inside class/trait/enum bodies (depth >= 1
    // or >= 2 under a braced namespace).
    let mut brace_depth: usize = 0;
    let mut namespace_brace_depth: Option<usize> = None;
    let mut pending: Option<(usize, usize, bool)> = None;
    let mut pending_is_group = false;

    for line in content.split('\n') {
        let line_brace_depth = brace_depth;

        // Brace-depth tracking is crude but sufficient — we only need an
        // approximate depth to tell top-level from class-body.  We skip
        // braces inside strings and comments only to the extent that
        // single-line `//` and `#` comments are trimmed, which covers
        // the vast majority of real-world PHP.
        let code = line.split("//").next().unwrap_or(line);
        let code = code.split('#').next().unwrap_or(code);

        let trimmed = line.trim_start();

        // Detect `namespace Foo {` so we know that depth 1 is still
        // "top-level" for use-import purposes.
        if trimmed.starts_with("namespace ") && code.contains('{') {
            // The opening brace on this line will bump brace_depth;
            // record that the namespace block starts at the *current*
            // depth (before the brace is counted).
            namespace_brace_depth = Some(brace_depth);
        }

        for ch in code.chars() {
            match ch {
                '{' => brace_depth += 1,
                '}' => {
                    brace_depth = brace_depth.saturating_sub(1);
                    // If we've closed the namespace block, clear the marker.
                    if namespace_brace_depth == Some(brace_depth) {
                        namespace_brace_depth = None;
                    }
                }
                _ => {}
            }
        }

        let top_level_depth = namespace_brace_depth.map_or(0, |d| d + 1);
        // A CRLF file's line still carries its `\r`; the statement ends
        // before it.
        let line_end = offset + line.trim_end_matches('\r').len();

        if let Some((line_start, keyword_start, top_level)) = pending {
            if trimmed.contains(';') {
                statements.push(UseStatementScan {
                    line_start,
                    keyword_start,
                    end: line_end,
                    top_level,
                });
                pending = None;
            } else if pending_is_group && trimmed.contains('}') {
                // A group import closes on `};`.  A `}` on its own means
                // the `use … {` we latched onto was a trait import with a
                // conflict-resolution block, so stop following it rather
                // than running on to some later statement's semicolon.
                pending = None;
            } else if trimmed.contains('{') {
                pending_is_group = true;
            }
        } else if trimmed.starts_with("use ") || trimmed.starts_with("use\t") {
            let keyword_start = offset + (line.len() - trimmed.len());
            let top_level = line_brace_depth == top_level_depth;
            if trimmed.contains(';') {
                statements.push(UseStatementScan {
                    line_start: offset,
                    keyword_start,
                    end: line_end,
                    top_level,
                });
            } else {
                pending = Some((offset, keyword_start, top_level));
                pending_is_group = trimmed.contains('{');
            }
        }

        offset += line.len() + 1; // +1 for the '\n' that `split` consumed
    }

    statements
}

/// Which byte ranges of the file are occupied by its namespace-level `use`
/// imports.
///
/// Answers "is this offset part of an import statement?" for callers that
/// need to suppress a diagnostic on the class name an import spells out.
/// Each range starts at the beginning of the statement's first line
/// (indentation included), so `is_offset_in_ranges` covers the whole line.
///
/// Deeper `use` statements are left out: a trait import inside a class body
/// is a real reference to the trait and must keep its diagnostics.
pub(crate) fn compute_use_line_ranges(content: &str) -> Vec<ByteRange> {
    scan_use_statements(content)
        .into_iter()
        .filter(|stmt| stmt.top_level)
        .map(|stmt| (stmt.line_start, stmt.end))
        .collect()
}

/// Where each of the file's `use` statements begins and ends.
///
/// Answers "which statements are the imports?" for callers that go on to
/// pick one apart with [`find_use_statement`].  Each span starts at the
/// `use` keyword (leading indentation excluded) and ends at the end of the
/// statement's last line.
///
/// Unlike [`compute_use_line_ranges`] this keeps the deeper statements
/// too, because a Blade template's `@php use App\Models\Post; @endphp`
/// lands inside the wrapper function of its virtual PHP and is still the
/// import a rename or an unused-import fix has to edit.  Callers pair this
/// with an alias from the file's import table, so a trait `use` in a class
/// body is only ever reached when no real import matched.
pub(crate) fn compute_use_statement_spans(content: &str) -> Vec<ByteRange> {
    scan_use_statements(content)
        .into_iter()
        .map(|stmt| (stmt.keyword_start, stmt.end))
        .collect()
}

/// Strip the leading `use` keyword and any `function` / `const` modifier
/// from a `use` statement, returning the remainder.
pub(crate) fn use_statement_body(stmt: &str) -> Option<&str> {
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
pub(crate) fn joined_name_matches(prefix: &str, name: &str, fqn: &str) -> bool {
    let name = name.trim_start_matches('\\');
    if prefix.is_empty() {
        return fqn == name;
    }
    fqn.len() == prefix.len() + 1 + name.len()
        && fqn.starts_with(prefix)
        && fqn.as_bytes()[prefix.len()] == b'\\'
        && fqn.ends_with(name)
}

/// Trim whitespace and comments from both ends of a group import member,
/// returning the span of what is left within `item`.
///
/// A wrapped group may carry comments between its members, as in
/// `Nonexistent, // could be namespace`, and the comment belongs to
/// whichever member `split(',')` happened to attach it to.
pub(crate) fn member_content_span(item: &str) -> (usize, usize) {
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

/// A match for one member of a `use` statement's import list: either a
/// group (`use Prefix\{Bar, Baz as B};`) or a plain, brace-less list
/// (`use Foo\Bar;` or `use Foo\Bar, Baz\Qux;`), with byte offsets relative
/// to the `decl` text passed to [`find_use_member`].
pub(crate) struct UseMemberMatch<'a> {
    pub(crate) start: usize,
    pub(crate) end: usize,
    /// How many items the statement has (a trailing comma does not add
    /// one). Always `1` for a plain single-class import.
    pub(crate) member_count: usize,
    /// The group's shared prefix (`Prefix` in `use Prefix\{...}`), with no
    /// leading or trailing `\`. Empty for a brace-less list, where each
    /// item already spells its own full name.
    pub(crate) prefix: &'a str,
}

/// Locate the item in a `use` statement's import list that imports `fqn`
/// under `alias`: a group member (`use Foo\{Bar, Baz as B};`), or one item
/// of a plain comma-separated list (including the ordinary single-import
/// case, `use Foo\Bar;`).
///
/// `decl` may span several lines, since PHP allows both shapes to be
/// wrapped.
pub(crate) fn find_use_member<'a>(
    decl: &'a str,
    fqn: &str,
    alias: &str,
) -> Option<UseMemberMatch<'a>> {
    let body = use_statement_body(decl)?;
    let body_start = decl.len() - body.len();

    // A group has a shared prefix ahead of its `{`; a plain list has no
    // prefix and its items span the whole body.
    let (prefix, items_start, items_end) = match decl.find('{') {
        Some(brace_open) => {
            let brace_close = decl.rfind('}')?;
            if brace_close < brace_open {
                return None;
            }
            // Everything between the `use` keyword and the `{` is the
            // shared prefix: `use App\Models\{` → `App\Models`.
            let prefix = decl
                .get(body_start..brace_open)?
                .trim()
                .trim_end_matches('\\')
                .trim_start_matches('\\');
            (prefix, brace_open + 1, brace_close)
        }
        None => ("", body_start, decl.len()),
    };

    let fqn = fqn.trim_start_matches('\\');

    let mut member_count = 0;
    let mut found = None;
    let mut offset = items_start;

    for item in decl[items_start..items_end].split(',') {
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
    Some(UseMemberMatch {
        start,
        end,
        member_count,
        prefix,
    })
}

/// Where a `use` import for `fqn` (imported as `alias`) sits in `content`:
/// the whole statement, plus the specific item within its import list that
/// names the class (which is the whole list for an ordinary single-class
/// import).
pub(crate) struct UseStatementLocation<'a> {
    /// The whole statement's byte range.
    pub(crate) statement: ByteRange,
    /// The item's byte range within `content` (already offset from the
    /// statement's start), covering any `as` clause.
    pub(crate) member: ByteRange,
    /// How many items the statement's import list has.
    pub(crate) member_count: usize,
    /// The group's shared prefix, with no leading or trailing `\`. Empty
    /// when the statement has no `{}` group.
    pub(crate) prefix: &'a str,
}

/// Find the `use` statement (and, within it, the specific list item) that
/// imports `fqn` under `alias` in `content`.
///
/// `use_statement_spans` is the result of [`compute_use_statement_spans`];
/// callers that already need it for another purpose pass it in rather than
/// have it recomputed here.
pub(crate) fn find_use_statement<'a>(
    content: &'a str,
    use_statement_spans: &[ByteRange],
    fqn: &str,
    alias: &str,
) -> Option<UseStatementLocation<'a>> {
    for &(stmt_start, stmt_end) in use_statement_spans {
        let Some(stmt) = content.get(stmt_start..stmt_end) else {
            continue;
        };

        // Match against the declaration only, so a trailing comment can't
        // be mistaken for a group body.
        let decl = stmt.split(';').next().unwrap_or(stmt);

        let Some(member) = find_use_member(decl, fqn, alias) else {
            continue;
        };
        return Some(UseStatementLocation {
            statement: (stmt_start, stmt_end),
            member: (stmt_start + member.start, stmt_start + member.end),
            member_count: member.member_count,
            prefix: member.prefix,
        });
    }

    None
}

#[cfg(test)]
mod tests {
    use super::{compute_use_line_ranges, compute_use_statement_spans, find_use_statement};

    #[test]
    fn use_line_ranges_lf() {
        let content = "<?php\nuse App\\Foo;\nnew Foo();\n";
        let ranges = compute_use_line_ranges(content);
        assert_eq!(ranges.len(), 1);
        let (start, end) = ranges[0];
        assert_eq!(&content[start..end], "use App\\Foo;");
    }

    /// The range lands exactly on the statement even though every line
    /// carries a two-byte `\r\n` terminator, and it stops before the `\r`.
    #[test]
    fn use_line_ranges_crlf() {
        let content = "<?php\r\nuse App\\Foo;\r\nnew Foo();\r\n";
        let ranges = compute_use_line_ranges(content);
        assert_eq!(ranges.len(), 1);
        let (start, end) = ranges[0];
        assert_eq!(&content[start..end], "use App\\Foo;");
    }

    /// A `use` inside a class body is a trait import, not a namespace
    /// import, and one under a braced namespace still counts.
    #[test]
    fn use_line_ranges_follow_brace_depth() {
        let content = "<?php\nnamespace App {\n    use Foo\\Bar;\n    class A {\n        use SomeTrait;\n    }\n}\n";
        let ranges = compute_use_line_ranges(content);
        assert_eq!(ranges.len(), 1);
        let (start, end) = ranges[0];
        assert_eq!(&content[start..end], "    use Foo\\Bar;");
    }

    /// A brace-less multi-import list wrapped across lines is still
    /// followed to its terminating `;` instead of being dropped.
    #[test]
    fn use_statement_spans_follow_a_wrapped_comma_list() {
        let content = "<?php\nuse App\\Models\\User,\n    App\\Models\\Post;\nnew User();\n";
        let spans = compute_use_statement_spans(content);
        assert_eq!(spans.len(), 1);
        let (start, end) = spans[0];
        assert_eq!(
            &content[start..end],
            "use App\\Models\\User,\n    App\\Models\\Post;"
        );
    }

    #[test]
    fn find_use_statement_locates_one_item_of_a_wrapped_comma_list() {
        let content = "<?php\nuse App\\Models\\User,\n    App\\Models\\Post;\nnew Post();\n";
        let spans = compute_use_statement_spans(content);
        let location =
            find_use_statement(content, &spans, "App\\Models\\Post", "Post").expect("located");
        assert_eq!(
            &content[location.member.0..location.member.1],
            "App\\Models\\Post"
        );
        assert_eq!(location.member_count, 2);
        assert_eq!(location.prefix, "");
    }

    #[test]
    fn find_use_statement_locates_a_group_member() {
        let content = "<?php\nuse App\\Models\\{User, Post};\n";
        let spans = compute_use_statement_spans(content);
        let location =
            find_use_statement(content, &spans, "App\\Models\\Post", "Post").expect("located");
        assert_eq!(&content[location.member.0..location.member.1], "Post");
        assert_eq!(location.member_count, 2);
        assert_eq!(location.prefix, "App\\Models");
    }
}
