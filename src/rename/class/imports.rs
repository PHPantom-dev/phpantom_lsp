//! How a file imports the class being renamed, and the edit that brings
//! that import up to date.

use std::collections::HashMap;

use tower_lsp::lsp_types::*;

use crate::text_position::offset_to_position;
use crate::util::strip_fqn_prefix;

pub(super) struct ImportInfo {
    /// The alias (short name) used in code.  For `use Ns\Foo;` this is
    /// `"Foo"`.  For `use Ns\Foo as Bar;` this is `"Bar"`.
    pub(super) alias: String,
    /// Whether an explicit `as` alias was used.
    pub(super) has_explicit_alias: bool,
}

/// Look up the import entry for a given FQN in a file's use_map.
///
/// The use_map is `alias → fqn`, so we need a reverse lookup.
pub(super) fn find_import_for_fqn(
    use_map: &HashMap<String, String>,
    target_fqn: &str,
) -> Option<ImportInfo> {
    let target_normalized = strip_fqn_prefix(target_fqn);
    let target_short = crate::util::short_name(target_normalized);

    for (alias, fqn) in use_map {
        let fqn_normalized = strip_fqn_prefix(fqn);
        if fqn_normalized.eq_ignore_ascii_case(target_normalized) {
            let has_explicit_alias = !alias.eq_ignore_ascii_case(target_short);
            return Some(ImportInfo {
                alias: alias.clone(),
                has_explicit_alias,
            });
        }
    }
    None
}

/// Whether a file declaring `file_namespace` resolves the short name of
/// `fqn` to `fqn` without needing a `use` import.
///
/// PHP falls back to the current namespace for unqualified class names,
/// so `namespace App\Support;` reaches `App\Support\Helper` as plain
/// `Helper`.  Namespace names are case-insensitive.
pub(super) fn namespace_owns(file_namespace: Option<&str>, fqn: &str) -> bool {
    let class_namespace = fqn.rfind('\\').map(|i| &fqn[..i]);
    match (file_namespace, class_namespace) {
        (Some(a), Some(b)) => a.eq_ignore_ascii_case(b),
        (None, None) => true,
        _ => false,
    }
}

/// Check whether importing `new_short_name` would collide with an
/// existing import in the file (other than the one being renamed).
pub(super) fn has_import_collision(
    use_map: &HashMap<String, String>,
    old_fqn: &str,
    new_short_name: &str,
) -> bool {
    let old_normalized = strip_fqn_prefix(old_fqn);
    let new_lower = new_short_name.to_lowercase();

    for (alias, fqn) in use_map {
        let fqn_normalized = strip_fqn_prefix(fqn);
        // Skip the entry for the class being renamed.
        if fqn_normalized.eq_ignore_ascii_case(old_normalized) {
            continue;
        }
        if alias.to_lowercase() == new_lower {
            return true;
        }
    }
    false
}

/// Pick an alias name to avoid a collision.
///
/// Tries `"{name}Alias"` first, then `"{name}Alias2"`, etc.  An alias is
/// a class name, so a candidate that differs from an existing import only
/// in casing still collides.
pub(super) fn pick_collision_alias(base_name: &str, use_map: &HashMap<String, String>) -> String {
    let is_free = |candidate: &str| {
        !use_map
            .keys()
            .any(|alias| alias.eq_ignore_ascii_case(candidate))
    };

    let candidate = format!("{}Alias", base_name);
    if is_free(&candidate) {
        return candidate;
    }
    for i in 2..100 {
        let candidate = format!("{}Alias{}", base_name, i);
        if is_free(&candidate) {
            return candidate;
        }
    }
    // Extremely unlikely fallback.
    format!("{}Alias99", base_name)
}

/// What a class is being renamed/moved to, bundled so
/// [`build_use_statement_edit`] stays under clippy's argument-count limit.
pub(super) struct RenameTarget<'a> {
    pub(super) new_fqn: &'a str,
    pub(super) new_short_name: &'a str,
    /// Whether the new short name collides with another import already in
    /// the file, so the rewritten import needs an alias.
    pub(super) has_collision: bool,
}

/// Plan the edit(s) needed to bring a file's `use` import for a renamed or
/// moved class up to date, together with the byte range those edits
/// already cover (so a per-reference edit can skip a location that falls
/// inside it instead of double-covering it).
///
/// Locates the import through the same statement scanner the unused-import
/// fixer uses ([`find_use_statement`] in `diagnostics/helpers`), so a group
/// import (`use App\Models\{User, Post};`) and a brace-less multi-import
/// list wrapped over several lines are both found, not just a whole
/// trimmed `use ... ;` line.
///
/// Two shapes are handled:
/// - A statement with only one item left after the rename (an ordinary
///   `use Old\Fqn [as Alias];`, or a group/list with a single member): one
///   `TextEdit` replacing the whole statement.
/// - One member of a multi-item group or list whose relative name still
///   fits under the shared prefix (empty for a brace-less list): a
///   `TextEdit` touching only that member. When it no longer fits (only
///   possible for a class *move*, which can change the namespace), the
///   member is dropped from the group instead and re-added as its own
///   `use` statement.
pub(super) fn build_use_statement_edit(
    content: &str,
    old_fqn: &str,
    target: &RenameTarget,
    info: &ImportInfo,
    use_map: &HashMap<String, String>,
    file_namespace: Option<&str>,
) -> Option<(Range, Vec<TextEdit>)> {
    let &RenameTarget {
        new_fqn,
        new_short_name,
        has_collision,
    } = target;

    // The import may spell the class in different casing than its
    // canonical declared FQN (PHP class names are case-insensitive), so
    // the statement is located by the exact text the file wrote rather
    // than by `old_fqn`.
    let source_fqn = use_map
        .get(&info.alias)
        .map(String::as_str)
        .unwrap_or(old_fqn);

    let use_statement_spans =
        crate::diagnostics::use_statements::compute_use_statement_spans(content);
    let location = crate::diagnostics::use_statements::find_use_statement(
        content,
        &use_statement_spans,
        source_fqn,
        &info.alias,
    )?;

    let whole_statement_edit = |range: Range| {
        let new_line = format!(
            "use {};",
            build_member_text(new_fqn, info, has_collision, new_short_name, use_map)
        );
        (
            range,
            vec![TextEdit {
                range,
                new_text: new_line,
            }],
        )
    };

    // A one-member group (or an ordinary single-class import) has nothing
    // left to keep, so it is rewritten as a plain statement the way a lone
    // `use Foo\Bar;` would be.
    if location.member_count <= 1 {
        let range = Range {
            start: offset_to_position(content, location.statement.0),
            end: offset_to_position(content, location.statement.1),
        };
        return Some(whole_statement_edit(range));
    }

    let member_range = Range {
        start: offset_to_position(content, location.member.0),
        end: offset_to_position(content, location.member.1),
    };

    if let Some(relative) = strip_matching_prefix(new_fqn, location.prefix) {
        let new_text = build_member_text(relative, info, has_collision, new_short_name, use_map);
        return Some((
            member_range,
            vec![TextEdit {
                range: member_range,
                new_text,
            }],
        ));
    }

    // The new namespace no longer fits the group's shared prefix (only
    // reachable from a class move): drop the member from the group and
    // re-add it as its own `use` statement.
    let (del_start, del_end) =
        list_item_delete_range(content, location.member.0, location.member.1);
    let delete_range = Range {
        start: offset_to_position(content, del_start),
        end: offset_to_position(content, del_end),
    };
    let mut edits = vec![TextEdit {
        range: delete_range,
        new_text: String::new(),
    }];

    let alias_for_new = if has_collision {
        Some(pick_collision_alias(new_short_name, use_map))
    } else if info.has_explicit_alias {
        Some(info.alias.clone())
    } else {
        None
    };
    let use_block = crate::completion::use_edit::analyze_use_block(content);
    let file_namespace_owned = file_namespace.map(str::to_string);
    if let Some(import_edits) = crate::completion::use_edit::build_aliased_use_edit(
        new_fqn,
        alias_for_new.as_deref(),
        &use_block,
        &file_namespace_owned,
    ) {
        edits.extend(import_edits);
    }

    let skip_range = Range {
        start: offset_to_position(content, location.statement.0),
        end: offset_to_position(content, location.statement.1),
    };
    Some((skip_range, edits))
}

/// Build the text for one `use` import target: either a whole statement's
/// FQN or one group member's relative name, with the `as` clause a
/// collision or an explicit alias requires.
fn build_member_text(
    name: &str,
    import_info: &ImportInfo,
    has_collision: bool,
    new_short_name: &str,
    use_map: &HashMap<String, String>,
) -> String {
    if has_collision {
        let alias = pick_collision_alias(new_short_name, use_map);
        format!("{} as {}", name, alias)
    } else if import_info.has_explicit_alias {
        format!("{} as {}", name, import_info.alias)
    } else {
        name.to_string()
    }
}

/// Strip a group import's shared prefix from `fqn`, comparing
/// case-insensitively (namespaces are case-insensitive in PHP).
///
/// Returns `None` when `fqn` does not fall under `prefix`, in which case
/// the class can no longer be named as a member of that group.
fn strip_matching_prefix<'a>(fqn: &'a str, prefix: &str) -> Option<&'a str> {
    // A brace-less list item (`use Foo\Bar, Baz\Qux;`) has no shared
    // prefix and already spells its own full name, with no separator to
    // consume.
    if prefix.is_empty() {
        return Some(fqn);
    }
    if !fqn.is_char_boundary(prefix.len()) {
        return None;
    }
    let (head, rest) = fqn.split_at(prefix.len());
    if !head.eq_ignore_ascii_case(prefix) {
        return None;
    }
    rest.strip_prefix('\\')
}

/// The byte range to delete from `content` to remove a comma-separated
/// list item spanning `item_start..item_end` (a group import member),
/// taking the trailing comma when present or the leading one otherwise,
/// along with the item's own indentation, so what remains reads as a valid
/// list with no blank line left where the item used to be.
fn list_item_delete_range(content: &str, item_start: usize, item_end: usize) -> (usize, usize) {
    let bytes = content.as_bytes();

    let mut start = item_start;
    while start > 0 && matches!(bytes[start - 1], b' ' | b'\t') {
        start -= 1;
    }

    let mut after = item_end;
    while bytes.get(after).is_some_and(u8::is_ascii_whitespace) {
        after += 1;
    }
    if bytes.get(after) == Some(&b',') {
        after += 1;
        while bytes.get(after).is_some_and(|b| matches!(b, b' ' | b'\t')) {
            after += 1;
        }
        if bytes.get(after) == Some(&b'\n') {
            after += 1;
        }
        return (start, after);
    }

    // No following comma: this was the last member, so the preceding one
    // goes instead.
    let mut before = start;
    while before > 0 && bytes[before - 1].is_ascii_whitespace() {
        before -= 1;
    }
    if before > 0 && bytes[before - 1] == b',' {
        before -= 1;
    }
    (before, item_end)
}
