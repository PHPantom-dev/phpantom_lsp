//! Use-statement insertion helpers.
//!
//! This module provides reusable helpers for computing where to insert a
//! `use` statement in a PHP file and for building the corresponding LSP
//! `TextEdit`.  These are shared by class-name completion.
//!
//! New `use` statements are inserted at the alphabetically correct
//! position among the existing imports so the use block stays sorted.
//!
//! A Blade template imports with the `@use` directive instead, and its
//! directives live in the template's own text rather than in the virtual
//! PHP the preprocessor lowers it to, so the block it takes is read and
//! written by [`crate::blade::use_block`].
use std::collections::HashMap;

use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::blade::use_block::TemplateUseBlock;
use crate::diagnostics::use_statements::scan_use_statements;
use crate::text_position::LineIndex;
use crate::text_scan::{HeaderEnd, code_follows_on_line, header_end, skip_php_comment};
use crate::util::short_name;

/// Where the first import of a block that has no `use` statement goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FirstImport {
    /// On a line of its own, starting at this one: the line after the
    /// `namespace` declaration, or after `<?php` and any `declare`.
    OwnLine(u32),
    /// Straight after the `{` or `;` that ends the `namespace` declaration,
    /// or after `<?php` and any `declare` in a file with none, because code
    /// follows it on the same line.  The line after it is past that code,
    /// and past the block itself when the block closes on the line.
    Inline(Position),
}

/// Information about a file's existing `use` block, used to compute
/// the correct alphabetical insertion position for new imports.
#[derive(Debug, Clone)]
pub(crate) struct UseBlockInfo {
    /// Each existing top-level `use` import: `(line_number, sort_key)`.
    /// `sort_key` is the lowercased FQN extracted from the statement,
    /// used for case-insensitive alphabetical comparison.
    /// Entries are in file order (sorted by line number).
    pub(crate) existing: Vec<(u32, String)>,
    /// Where to insert when there are no existing `use` statements.
    pub(crate) fallback: FirstImport,
    /// Whether the file declares a namespace.  When there are no
    /// existing imports, a blank line is inserted before the first
    /// `use` statement to separate it from the `namespace` line, unless
    /// the import is written inline after the declaration.
    pub(crate) has_namespace: bool,
    /// The template's own import block, when the file is a Blade
    /// template rather than a PHP file.
    pub(crate) template: Option<TemplateUseBlock>,
}

impl UseBlockInfo {
    /// Compute the insertion `Position` for a new `use` statement that
    /// imports the given FQN, maintaining alphabetical order among the
    /// existing imports.
    ///
    /// If there are no existing imports, returns the fallback position
    /// (after the `namespace` declaration or `<?php`).
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
            return match self.fallback {
                FirstImport::OwnLine(line) => Position { line, character: 0 },
                FirstImport::Inline(position) => position,
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

    /// Whether an import is written right after the `namespace`
    /// declaration or the file header instead of on a line of its own: the
    /// block has no imports yet, and code follows on the same line.
    fn writes_inline(&self) -> bool {
        self.existing.is_empty() && matches!(self.fallback, FirstImport::Inline(_))
    }

    /// The text of the edit that writes `statement` at the position
    /// [`insert_position_for_key`](Self::insert_position_for_key) names.
    ///
    /// An import on a line of its own ends with a line break, and `set_off`
    /// puts a blank line before it to separate it from what precedes.  One
    /// written inline starts with a space instead, which sets it off from
    /// the declaration it follows.
    pub(crate) fn import_text(&self, statement: &str, set_off: bool) -> String {
        if self.writes_inline() {
            format!(" {statement}")
        } else if set_off {
            format!("\n{statement}\n")
        } else {
            format!("{statement}\n")
        }
    }

    /// Drop the blank line [`build_use_edit`] puts between the `namespace`
    /// line and the first import of a block that has none, from an import
    /// that is not the first of its batch.
    ///
    /// Every import of a batch is planned against the same empty block, so
    /// each would add the separator.  A template's import keeps the line
    /// break it starts with: there it separates the directive from the
    /// line it follows.  An import written inline has no separator to
    /// drop.
    pub(crate) fn drop_repeated_separator(&self, edits: &mut [TextEdit]) {
        if self.template.is_some() || !self.existing.is_empty() {
            return;
        }
        for edit in edits {
            if let Some(rest) = edit.new_text.strip_prefix('\n') {
                edit.new_text = rest.to_string();
            }
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
pub(crate) fn extract_use_sort_key(line: &str) -> Option<String> {
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
/// This answers "where does a *new* import go?".  To find the statement
/// that already imports a given class, use
/// [`crate::diagnostics::use_statements::find_use_statement`] instead.
///
/// The imports are read by [`scan_use_statements`], which tells a
/// namespace-level import from a trait `use` inside a class, enum, or
/// trait body.
pub(crate) fn analyze_use_block(content: &str) -> UseBlockInfo {
    analyze_use_block_in(content, None)
}

/// [`analyze_use_block`] for one `namespace` block of a file that
/// declares several, given as its byte range, or for the whole file when
/// `block` is `None`.
///
/// PHP scopes an import to its block, so a new import joins the `use`
/// statements of the block the code needing it is written in, and goes
/// right after that block's own `namespace` declaration when it has none.
pub(crate) fn analyze_use_block_in(content: &str, block: Option<(usize, usize)>) -> UseBlockInfo {
    let index = LineIndex::new(content);

    // The block's range starts at its `namespace` keyword; its imports
    // follow the `;` or `{` that ends the declaration.
    let keyword = match block {
        Some((start, _)) => Some(start),
        None => last_namespace_keyword(content),
    };
    let declaration_end = keyword.and_then(|keyword| namespace_declaration_end(content, keyword));

    let existing = scan_use_statements(content)
        .into_iter()
        .filter(|statement| statement.top_level)
        .filter(|statement| {
            block.is_none_or(|(start, end)| {
                statement.keyword_start >= start && statement.keyword_start <= end
            })
        })
        .filter_map(|statement| {
            let sort_key = extract_use_sort_key(&content[statement.keyword_start..statement.end])?;
            Some((index.position(statement.line_start).line, sort_key))
        })
        .collect();

    // Fallback: insert after the `namespace` declaration, on the next line
    // or, when code follows the declaration on its own line, straight after
    // it, since the next line is then past that code.  With no namespace,
    // just past the file's header, which is past any
    // `declare(strict_types=1)`, since PHP requires that to come first.
    let fallback = match (declaration_end, keyword) {
        (Some(end), _) if code_follows_on_line(content, end) => {
            FirstImport::Inline(index.position(end))
        }
        (Some(end), _) => FirstImport::OwnLine(index.position(end).line + 1),
        (None, Some(keyword)) => FirstImport::OwnLine(index.position(keyword).line + 1),
        (None, None) => match header_end(content) {
            HeaderEnd::Line(line) => FirstImport::OwnLine(line),
            HeaderEnd::Inline(end) => FirstImport::Inline(index.position(end)),
        },
    };

    UseBlockInfo {
        existing,
        fallback,
        has_namespace: keyword.is_some(),
        template: None,
    }
}

/// The byte offset of the `namespace` keyword of the last line of
/// `content` that declares one, which may follow the opening tag.
///
/// A line starting `namespace\something` is a different construct, the
/// relative name, and does not count.
fn last_namespace_keyword(content: &str) -> Option<usize> {
    let mut found = None;
    let mut line_start = 0;
    for line in content.split_inclusive('\n') {
        let mut code = line.trim_start();
        if let Some(after_tag) = code.strip_prefix("<?php") {
            code = after_tag.trim_start();
        }
        if code.starts_with("namespace ") || code.starts_with("namespace\t") {
            found = Some(line_start + line.len() - code.len());
        }
        line_start += line.len();
    }
    found
}

/// The byte offset just past the `;` or `{` that ends the `namespace`
/// declaration whose keyword starts at `keyword`.
///
/// `None` when what follows the keyword is not a declaration: only a name
/// and comments may sit between the keyword and its terminator.
fn namespace_declaration_end(content: &str, keyword: usize) -> Option<usize> {
    let bytes = content.as_bytes();
    let mut at = keyword + "namespace".len();
    while let Some(&byte) = bytes.get(at) {
        at = match byte {
            b';' | b'{' => return Some(at + 1),
            b'\\' | b'_' | 0x80.. => at + 1,
            _ if byte.is_ascii_alphanumeric() || byte.is_ascii_whitespace() => at + 1,
            _ => skip_php_comment(bytes, at)?,
        };
    }
    None
}

impl Backend {
    /// The use block a new import for `uri` joins: the file's own `use`
    /// statements, or a template's `@use` directives.
    ///
    /// `content` is the text the caller analysed, which for a template is
    /// the virtual PHP it lowers to.  A template's imports were hoisted
    /// into the prologue of that text, so the template itself is scanned
    /// instead ([`crate::blade::use_block::analyze_template_use_block`]).
    ///
    /// `block` is the byte range of the `namespace` block the import is for
    /// (see [`ImportBlock`](crate::backend::file_access::ImportBlock)), or
    /// `None` when the file has only one.
    pub(crate) fn use_block_for(
        &self,
        uri: &str,
        content: &str,
        block: Option<(usize, usize)>,
    ) -> UseBlockInfo {
        if !self.is_blade_file(uri) {
            return analyze_use_block_in(content, block);
        }
        let Some(template) = self.get_file_content_arc(uri) else {
            return analyze_use_block_in(content, block);
        };
        let maps = self.blade_source_maps.read();
        crate::blade::use_block::analyze_template_use_block(&template, maps.get(uri))
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

    if let Some(template) = &use_block.template {
        return Some(vec![template.import_edit(
            &use_block.existing,
            &fqn.to_lowercase(),
            fqn,
            alias,
        )]);
    }

    let insert_pos = use_block.insert_position_for(fqn);

    // When there are no existing imports and the file has a namespace,
    // a blank line separates the namespace declaration from the use block.
    let set_off = use_block.existing.is_empty() && use_block.has_namespace;

    let statement = match alias {
        Some(alias) => format!("use {} as {};", fqn, alias),
        None => format!("use {};", fqn),
    };

    Some(vec![TextEdit {
        range: Range {
            start: insert_pos,
            end: insert_pos,
        },
        new_text: use_block.import_text(&statement, set_off),
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

    if let Some(template) = &use_block.template {
        return Some(vec![template.import_edit(
            &use_block.existing,
            &sort_key,
            &format!("{kind} {fqn}"),
            alias,
        )]);
    }

    let insert_pos = use_block.insert_position_for_key(&sort_key);

    // Set the import off with a blank line when:
    // - There are no existing imports at all and the file has a
    //   namespace (separate namespace from the use block), or
    // - This is the first function import and there are already class
    //   imports (group separator).
    let has_kind_imports = use_block
        .existing
        .iter()
        .any(|(_, key)| key.starts_with(kind));
    let set_off = (use_block.existing.is_empty() && use_block.has_namespace)
        || (!has_kind_imports && use_block.has_class_imports());

    let statement = match alias {
        Some(alias) => format!("use {} {} as {};", kind, fqn, alias),
        None => format!("use {} {};", kind, fqn),
    };

    Some(vec![TextEdit {
        range: Range {
            start: insert_pos,
            end: insert_pos,
        },
        new_text: use_block.import_text(&statement, set_off),
    }])
}

#[cfg(test)]
#[path = "use_edit_tests.rs"]
mod tests;
