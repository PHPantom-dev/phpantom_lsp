//! Use-statement insertion helpers.
//!
//! This module provides reusable helpers for computing where to insert a
//! `use` statement in a PHP file and for building the corresponding LSP
//! `TextEdit`.  These are shared by class-name completion.
//!
//! New `use` statements are inserted at the alphabetically correct
//! position among the existing imports so the use block stays sorted.
use std::collections::HashMap;

use tower_lsp::lsp_types::*;

use crate::util::short_name;

/// Information about a file's existing `use` block, used to compute
/// the correct alphabetical insertion position for new imports.
#[derive(Debug, Clone)]
pub(crate) struct UseBlockInfo {
    /// Each existing top-level `use` import: `(line_number, sort_key)`.
    /// `sort_key` is the lowercased FQN extracted from the statement,
    /// used for case-insensitive alphabetical comparison.
    /// Entries are in file order (sorted by line number).
    pub(crate) existing: Vec<(u32, String)>,
    /// The line to insert at when there are no existing `use` statements.
    /// Points after the `namespace` declaration, or after `<?php`.
    pub(crate) fallback_line: u32,
    /// Whether the file declares a namespace.  When there are no
    /// existing imports, a blank line is inserted before the first
    /// `use` statement to separate it from the `namespace` line.
    pub(crate) has_namespace: bool,
}

impl UseBlockInfo {
    /// Compute the insertion `Position` for a new `use` statement that
    /// imports the given FQN, maintaining alphabetical order among the
    /// existing imports.
    ///
    /// If there are no existing imports, returns the fallback position
    /// (after `namespace` or `<?php`).
    pub(crate) fn insert_position_for(&self, fqn: &str) -> Position {
        self.insert_position_for_key(&fqn.to_lowercase())
    }

    /// Like [`insert_position_for`](Self::insert_position_for) but
    /// accepts a pre-computed sort key instead of deriving one from the
    /// FQN.  This is useful for `use function` and `use const` imports
    /// whose sort keys carry a `"function "` or `"const "` prefix so
    /// they sort into their own group.
    ///
    /// Import statements are organized into three groups that never
    /// interleave:
    ///
    ///   1. **Class** imports (bare `use Foo\Bar;`)
    ///   2. **Const** imports (`use const Foo\BAR;`)
    ///   3. **Function** imports (`use function Foo\bar;`)
    ///
    /// Within each group the imports are sorted alphabetically.  When
    /// inserting into a group that already has entries, the new import
    /// is placed at the correct alphabetical position inside that
    /// group.  When the target group is empty, the import is placed
    /// after the last entry of a lower-priority group (or before the
    /// first entry of a higher-priority group if no lower group
    /// exists).
    pub(crate) fn insert_position_for_key(&self, key: &str) -> Position {
        if self.existing.is_empty() {
            return Position {
                line: self.fallback_line,
                character: 0,
            };
        }

        let new_group = Self::key_group(key);

        // Collect entries that belong to the same group.
        let same_group: Vec<&(u32, String)> = self
            .existing
            .iter()
            .filter(|(_, k)| Self::key_group(k) == new_group)
            .collect();

        if !same_group.is_empty() {
            // Insert alphabetically within the group.
            for (line, existing_key) in &same_group {
                if existing_key.as_str() > key {
                    return Position {
                        line: *line,
                        character: 0,
                    };
                }
            }
            // Sorts after every entry in the group — append after the last one.
            let last_line = same_group.last().expect("non-empty").0;
            return Position {
                line: last_line + 1,
                character: 0,
            };
        }

        // The target group has no entries yet.  Place after the last
        // entry of a lower-priority group, or before the first entry
        // of a higher-priority group.
        let lower: Vec<&(u32, String)> = self
            .existing
            .iter()
            .filter(|(_, k)| Self::key_group(k) < new_group)
            .collect();

        if let Some(&&(last_line, _)) = lower.last() {
            return Position {
                line: last_line + 1,
                character: 0,
            };
        }

        // No lower-priority group — insert before the very first import.
        let first_line = self.existing.first().expect("non-empty checked above").0;
        Position {
            line: first_line,
            character: 0,
        }
    }

    /// Determine which group a sort key belongs to.
    ///
    /// Group ordering: class (0) < const (1) < function (2).
    pub(crate) fn key_group(key: &str) -> u8 {
        if key.starts_with("function ") {
            2
        } else if key.starts_with("const ") {
            1
        } else {
            0
        }
    }

    /// Check whether the existing use block contains any class (plain
    /// `use`) imports — i.e. imports that are neither `use function`
    /// nor `use const`.
    pub(crate) fn has_class_imports(&self) -> bool {
        self.existing.iter().any(|(_, k)| Self::key_group(k) == 0)
    }

    /// Check whether a `use function` import already exists whose short
    /// name (case-insensitive) matches the given short name but whose
    /// FQN differs from `fqn`.  This indicates a conflict: the short
    /// name is already taken by a different function.
    pub(crate) fn function_import_conflicts(&self, fqn: &str) -> bool {
        let short = crate::util::short_name(fqn).to_lowercase();
        let target_key = format!("function {}", fqn.to_lowercase());
        self.existing.iter().any(|(_, k)| {
            if !k.starts_with("function ") {
                return false;
            }
            if k == &target_key {
                // Same FQN — not a conflict, it's already imported.
                return false;
            }
            // Extract short name from the sort key.
            let existing_fqn = k.strip_prefix("function ").unwrap_or(k);
            let existing_short = existing_fqn.rsplit('\\').next().unwrap_or(existing_fqn);
            existing_short == short
        })
    }
}

/// Extract the sort key (lowercased FQN) from a `use` statement line.
///
/// Handles the common forms:
///   - `use Foo\Bar;` → `foo\bar`
///   - `use Foo\Bar as Alias;` → `foo\bar`
///   - `use function Foo\bar;` → `function foo\bar` (preserves keyword prefix for grouping)
///   - `use const Foo\BAR;` → `const foo\bar`
///   - `use Foo\{Bar, Baz};` → `foo\`
///
/// Returns `None` if the line does not look like a use statement.
fn extract_use_sort_key(line: &str) -> Option<String> {
    let trimmed = line.trim();
    let rest = trimmed
        .strip_prefix("use ")
        .or_else(|| trimmed.strip_prefix("use\t"))?;

    // Skip `use (` / `use(` — those are closures, not imports.
    if rest.starts_with('(') {
        return None;
    }

    // Preserve `function`/`const` prefix so they sort into their own
    // group naturally (all `const …` together, all `function …` together).
    let (prefix, fqn_part) = if let Some(r) = rest.strip_prefix("function ") {
        ("function ", r)
    } else if let Some(r) = rest.strip_prefix("const ") {
        ("const ", r)
    } else {
        ("", rest)
    };

    // Extract the FQN: everything up to `;`, ` as `, or `{`.
    let fqn = fqn_part
        .split(';')
        .next()
        .unwrap_or(fqn_part)
        .split(" as ")
        .next()
        .unwrap_or(fqn_part)
        .split('{')
        .next()
        .unwrap_or(fqn_part)
        .trim()
        .trim_start_matches('\\');

    Some(format!("{}{}", prefix, fqn).to_lowercase())
}

/// Analyse the file content and return a [`UseBlockInfo`] describing the
/// existing `use` block, which supports alphabetical insertion via
/// [`UseBlockInfo::insert_position_for`].
///
/// The scanning logic distinguishes top-level `use` imports from trait
/// `use` statements inside class/enum/trait bodies by tracking brace
/// depth.
pub(crate) fn analyze_use_block(content: &str) -> UseBlockInfo {
    let mut existing: Vec<(u32, String)> = Vec::new();
    let mut namespace_line: Option<u32> = None;
    let mut php_open_line: Option<u32> = None;

    // Track brace depth so we can distinguish top-level `use` imports
    // from trait `use` statements inside class/enum/trait bodies.
    //
    // With semicolon-style namespaces (`namespace Foo;`), imports live
    // at depth 0 and class bodies are at depth 1.
    //
    // With brace-style namespaces (`namespace Foo { ... }`), imports
    // live at depth 1 and class bodies are at depth 2.
    //
    // We compute depth at the START of each line and track whether we
    // saw a brace-style namespace to set the right threshold.
    let mut brace_depth: u32 = 0;
    let mut uses_brace_namespace = false;

    for (i, line) in content.lines().enumerate() {
        let trimmed = line.trim();

        // The depth at the start of this line (before counting its braces).
        let depth_at_start = brace_depth;

        // Update brace depth for the NEXT line.
        for ch in trimmed.chars() {
            match ch {
                '{' => brace_depth += 1,
                '}' => brace_depth = brace_depth.saturating_sub(1),
                _ => {}
            }
        }

        if trimmed.starts_with("<?php") && php_open_line.is_none() {
            php_open_line = Some(i as u32);
        }

        // Match `namespace Foo\Bar;` or `namespace Foo\Bar {`
        // but not `namespace\something` (which is a different construct).
        if trimmed.starts_with("namespace ") || trimmed.starts_with("namespace\t") {
            namespace_line = Some(i as u32);
            if trimmed.contains('{') {
                uses_brace_namespace = true;
            }
        }

        // The maximum brace depth at which `use` statements are still
        // namespace imports (not trait imports inside a class body).
        let max_import_depth = if uses_brace_namespace { 1 } else { 0 };

        // Match `use Foo\Bar;`, `use Foo\{Bar, Baz};`, etc.
        // Only at the import level — deeper means trait `use` inside a
        // class/enum/trait body.
        if depth_at_start <= max_import_depth
            && (trimmed.starts_with("use ") || trimmed.starts_with("use\t"))
            && !trimmed.starts_with("use (")
            && !trimmed.starts_with("use(")
            && let Some(sort_key) = extract_use_sort_key(trimmed)
        {
            existing.push((i as u32, sort_key));
        }
    }

    // Fallback: insert after `namespace`, or after `<?php`.
    let fallback_line = namespace_line.or(php_open_line).map(|l| l + 1).unwrap_or(0);
    let has_namespace = namespace_line.is_some();

    UseBlockInfo {
        existing,
        fallback_line,
        has_namespace,
    }
}

/// Check whether importing the given FQN would create a conflict with an
/// existing `use` statement in the file.
///
/// Two kinds of conflict are detected (both case-insensitive):
///
/// 1. **Short-name collision.** The short name of the FQN (the part after
///    the last `\`) matches an alias that already points to a different
///    class.  For example, `use Cassandra\Exception;` blocks importing
///    `App\Exception` because both resolve to the alias `Exception`.
///
/// 2. **Leading-segment collision.** The first namespace segment of the
///    FQN matches an existing alias.  For example, `use Stringable as pq;`
///    blocks importing `pq\Exception` because writing `pq\Exception` in
///    code would resolve `pq` through the alias, not through the
///    namespace.
pub(crate) fn use_import_conflicts(fqn: &str, file_use_map: &HashMap<String, String>) -> bool {
    let sn = short_name(fqn);
    // The first namespace segment (e.g. `pq` in `pq\Exception`).
    // For single-segment FQNs this equals the short name, so the
    // leading-segment check is redundant with the short-name check and
    // we skip it to avoid a false positive against the class's own
    // import.
    let first_segment = fqn.split('\\').next().unwrap_or(fqn);
    let has_namespace = fqn.contains('\\');

    for (alias, existing_fqn) in file_use_map {
        // 1. Short-name collision.
        if alias.eq_ignore_ascii_case(sn) && !existing_fqn.eq_ignore_ascii_case(fqn) {
            return true;
        }
        // 2. Leading-segment collision (only for multi-segment FQNs).
        if has_namespace && alias.eq_ignore_ascii_case(first_segment) {
            return true;
        }
    }
    false
}

/// Build an `additional_text_edits` entry that inserts a `use` statement
/// for the given fully-qualified class name at the alphabetically correct
/// position in the file's existing use block.
///
/// When the FQN has no namespace separator (e.g. `PDO`, `DateTime`),
/// an import is only needed if the current file declares a namespace —
/// otherwise we are already in the global namespace and no `use`
/// statement is required.  Returns `None` in that case.
///
/// When there are no existing `use` statements and the file declares a
/// namespace, a blank line (`\n`) is prepended to separate the new
/// import from the `namespace` declaration.
pub(crate) fn build_use_edit(
    fqn: &str,
    use_block: &UseBlockInfo,
    file_namespace: &Option<String>,
) -> Option<Vec<TextEdit>> {
    build_aliased_use_edit(fqn, None, use_block, file_namespace)
}

/// Like [`build_use_edit`] but emits `use Ns\Foo as Alias;` when `alias`
/// is `Some`.  Used by the class-move rename, which has to import a
/// moved class under an alias when its short name is already taken in
/// the importing file.
pub(crate) fn build_aliased_use_edit(
    fqn: &str,
    alias: Option<&str>,
    use_block: &UseBlockInfo,
    file_namespace: &Option<String>,
) -> Option<Vec<TextEdit>> {
    // No namespace separator → this is a global class (e.g. `PDO`, `DateTime`).
    // Only needs an import when the current file declares a namespace;
    // otherwise we're already in the global namespace.
    if !fqn.contains('\\') && file_namespace.is_none() {
        return None;
    }

    let insert_pos = use_block.insert_position_for(fqn);

    // When there are no existing imports and the file has a namespace,
    // prepend a blank line to separate the namespace declaration from
    // the use block.
    let prefix = if use_block.existing.is_empty() && use_block.has_namespace {
        "\n"
    } else {
        ""
    };

    let statement = match alias {
        Some(alias) => format!("use {} as {};", fqn, alias),
        None => format!("use {};", fqn),
    };

    Some(vec![TextEdit {
        range: Range {
            start: insert_pos,
            end: insert_pos,
        },
        new_text: format!("{}{}\n", prefix, statement),
    }])
}

/// Build an `additional_text_edits` entry that inserts a `use function`
/// statement for the given fully-qualified function name at the
/// alphabetically correct position in the file's existing use block.
///
/// The sort key is prefixed with `"function "` so that function imports
/// naturally group after class imports and among other function imports.
/// When this is the first `use function` being added and there are
/// existing class imports, a blank line is prepended to visually
/// separate the two groups (matching PSR-12 / Laravel conventions).
///
/// Only produces an edit when the function is namespaced (contains `\`).
/// Global functions never need importing.  Returns `None` when no import
/// is required.
pub(crate) fn build_use_function_edit(
    fqn: &str,
    use_block: &UseBlockInfo,
) -> Option<Vec<TextEdit>> {
    build_aliased_typed_use_edit(fqn, None, "function", use_block)
}

/// Build a `use function` or `use const` edit, optionally under an alias.
pub(crate) fn build_aliased_typed_use_edit(
    fqn: &str,
    alias: Option<&str>,
    kind: &str,
    use_block: &UseBlockInfo,
) -> Option<Vec<TextEdit>> {
    // Global functions (no namespace separator) never need importing.
    if !fqn.contains('\\') {
        return None;
    }

    let sort_key = format!("{} {}", kind, fqn.to_lowercase());

    // Skip if this exact function is already imported.
    if use_block.existing.iter().any(|(_, k)| k == &sort_key) {
        return None;
    }
    let insert_pos = use_block.insert_position_for_key(&sort_key);

    // Prepend a blank line when:
    // - There are no existing imports at all and the file has a
    //   namespace (separate namespace from the use block), or
    // - This is the first function import and there are already class
    //   imports (group separator).
    let has_kind_imports = use_block
        .existing
        .iter()
        .any(|(_, key)| key.starts_with(kind));
    let separator = if (use_block.existing.is_empty() && use_block.has_namespace)
        || (!has_kind_imports && use_block.has_class_imports())
    {
        "\n"
    } else {
        ""
    };

    Some(vec![TextEdit {
        range: Range {
            start: insert_pos,
            end: insert_pos,
        },
        new_text: match alias {
            Some(alias) => format!("{}use {} {} as {};\n", separator, kind, fqn, alias),
            None => format!("{}use {} {};\n", separator, kind, fqn),
        },
    }])
}

#[cfg(test)]
#[path = "use_edit_tests.rs"]
mod tests;
