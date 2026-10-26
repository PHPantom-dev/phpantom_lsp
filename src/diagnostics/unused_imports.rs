//! Unused `use` statement dimming.
//!
//! After `update_ast`, compare each `use` declaration against all symbol
//! references in the file.  Any import alias that has zero references
//! gets a diagnostic with `Severity::Hint` and `DiagnosticTag::Unnecessary`,
//! which editors render as dimmed text.
//!
//! We only check class-level `use` imports, including `use function` and
//! `use const`, but not trait `use` inside class bodies.  A Blade template's
//! `@use` directives are imports too, reported at the directive.

use std::collections::HashSet;

use tower_lsp_server::gen_lsp_types::*;

use crate::Backend;
use crate::backend::file_access::ImportBlock;
use crate::blade::use_directive::find_use_directive;
use crate::symbol_map::SymbolKind;
use crate::text_position::byte_range_to_lsp_range;

use super::helpers::{ByteRange, is_offset_in_ranges, make_tagged_diagnostic};
use super::use_statements::{
    compute_use_line_ranges, compute_use_statement_spans, find_use_statement,
};

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
        // ── Gather each `namespace` block's imports (short name → FQN) ──
        // PHP scopes an import to the block that declares it, so each
        // block's imports are matched against the references written in
        // that block alone.
        let blocks = self.import_blocks(uri);

        if blocks
            .iter()
            .all(|block| block.use_map.is_empty() && block.symbol_imports.is_empty())
        {
            return;
        }

        // ── Gather the symbol map ───────────────────────────────────────
        // `content` must be the text the map was extracted from: a Blade
        // template's map describes the virtual PHP it lowers to, and every
        // offset below (the span slices, the `use`-line ranges) is read
        // against the same string.
        let Some(symbol_map) = self.symbol_map_for(uri) else {
            return;
        };
        let Some(source) = symbol_map.source(content) else {
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
        //
        // One entry per block, holding the aliases referenced inside it.
        let mut referenced_aliases: Vec<ReferencedAliases<'_>> = (0..blocks.len())
            .map(|_| ReferencedAliases::default())
            .collect();

        for span in &symbol_map.spans {
            // Skip spans that fall on `use` statement lines — those are
            // the import declarations, not actual usage sites.
            if is_offset_in_ranges(span.start, &use_line_ranges) {
                continue;
            }

            // Which import table a bare name is looked up in.  A qualified
            // name's first segment is always a namespace, imported with a
            // class-style `use`.
            let (name, kind) = match &span.kind {
                // The name may be fully qualified, partially qualified,
                // or unqualified.  We need to check if the first segment
                // (or the whole name for unqualified) matches a use alias.
                SymbolKind::ClassReference { name, .. } => (name.as_str(), ImportKind::Class),

                SymbolKind::MemberAccess {
                    subject_text,
                    is_static: true,
                    ..
                } => {
                    // Static access: `Foo::bar()` — subject_text is `"Foo"`
                    let trimmed = subject_text.as_str(source).trim();
                    if trimmed.starts_with('$')
                        || trimmed == "self"
                        || trimmed == "static"
                        || trimmed == "parent"
                    {
                        continue;
                    }
                    (trimmed, ImportKind::Class)
                }

                SymbolKind::FunctionCall { name, .. } => (name.as_str(), ImportKind::Function),

                SymbolKind::ConstantReference { name, .. } => (name.as_str(), ImportKind::Constant),

                _ => continue,
            };

            let index = ImportBlock::index_at(&blocks, span.start as usize);
            let block = &blocks[index];
            let referenced = &mut referenced_aliases[index];
            let first_segment = extract_first_segment(name);
            let (table, set) = match kind {
                _ if first_segment.len() != name.len() => (&block.use_map, &mut referenced.classes),
                ImportKind::Class => (&block.use_map, &mut referenced.classes),
                ImportKind::Function => {
                    (&block.symbol_imports.functions, &mut referenced.functions)
                }
                ImportKind::Constant => {
                    (&block.symbol_imports.constants, &mut referenced.constants)
                }
            };
            if let Some((alias, _)) = table.get_key_value(first_segment) {
                set.insert(alias.as_str());
            }
        }

        for (block, referenced) in blocks.iter().zip(&referenced_aliases) {
            self.report_unused_imports_in_block(
                uri,
                content,
                block,
                referenced,
                &use_statement_spans,
                out,
            );
        }
    }

    /// Report the imports of one `namespace` block that the symbol map
    /// (`referenced`) finds no use of inside the block.
    fn report_unused_imports_in_block(
        &self,
        uri: &str,
        content: &str,
        block: &ImportBlock,
        referenced: &ReferencedAliases<'_>,
        use_statements: &[ByteRange],
        out: &mut Vec<Diagnostic>,
    ) {
        // Filter to only aliases the symbol map didn't find.
        let imports = &block.symbol_imports;
        let unused_aliases: Vec<(&String, &String)> = [
            (&block.use_map, &referenced.classes),
            (&imports.functions, &referenced.functions),
            (&imports.constants, &referenced.constants),
        ]
        .into_iter()
        .flat_map(|(table, seen)| {
            table
                .iter()
                .filter(|(alias, _)| !seen.contains(alias.as_str()))
        })
        .collect();

        if unused_aliases.is_empty() {
            return;
        }

        let search_range = block.range.unwrap_or((0, content.len()));

        // Only this block's own statements: another block importing the
        // same name has its own statement, reported with that block.
        let block_statements: Vec<ByteRange> = use_statements
            .iter()
            .copied()
            .filter(|&(start, _)| start >= search_range.0 && start <= search_range.1)
            .collect();

        // A template's `@use` directives are hoisted into the prologue of its
        // virtual PHP, which no template text stands behind, so one nothing
        // uses is reported where the template wrote it.  The template is
        // read once an import turns out to be unused.
        let mut template = None;

        for (alias, fqn) in unused_aliases {
            let template = template.get_or_insert_with(|| {
                self.is_blade_file(uri)
                    .then(|| self.get_file_content_arc(uri))
                    .flatten()
            });
            let range = template
                .as_deref()
                .and_then(|template| use_directive_range(template, fqn, alias))
                .or_else(|| {
                    find_use_statement_range(self, uri, content, alias, fqn, &block_statements)
                });
            if let Some(range) = range {
                out.push(make_tagged_diagnostic(
                    range,
                    DiagnosticSeverity::Hint,
                    "unused_import",
                    format!("Unused import '{}'", fqn),
                    Some(DiagnosticTag::Unnecessary),
                ));
            }
        }
    }
}

/// The kind of import a reference can use, by the symbol table PHP looks
/// its name up in.
#[derive(Clone, Copy)]
enum ImportKind {
    Class,
    Function,
    Constant,
}

/// The aliases one `namespace` block's references use, per import kind.
#[derive(Default)]
struct ReferencedAliases<'a> {
    classes: HashSet<&'a str>,
    functions: HashSet<&'a str>,
    constants: HashSet<&'a str>,
}

// ─── Helpers ────────────────────────────────────────────────────────────────

/// Extract the first segment of a potentially qualified name.
///
/// - `"Foo"` → `"Foo"`
/// - `"Foo\\Bar"` → `"Foo"`
fn extract_first_segment(name: &str) -> &str {
    name.split('\\').next().unwrap_or(name)
}

/// The range of the `@use` directive in `template`, a Blade template's own
/// text, that imports `fqn` as `alias`.
fn use_directive_range(template: &str, fqn: &str, alias: &str) -> Option<Range> {
    let span = find_use_directive(template, fqn, alias)?;
    Some(byte_range_to_lsp_range(template, span.start, span.end))
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
    let location = find_use_statement(content, use_ranges, fqn, alias)?;
    // A one-member group (or an ordinary single-class import) has nothing
    // left to keep, so the whole statement is highlighted; otherwise just
    // the unused member.
    let (start, end) = if location.member_count > 1 {
        location.member
    } else {
        location.statement
    };
    backend.offset_range_to_lsp_range(uri, content, start, end)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics::use_statements::find_use_member;

    // ── Group import member lookup ──────────────────────────────────

    #[test]
    fn finds_member_in_single_line_group() {
        let decl = "use App\\Models\\{User, Post}";
        let m = find_use_member(decl, "App\\Models\\Post", "Post").unwrap();
        assert_eq!(&decl[m.start..m.end], "Post");
        assert_eq!(m.member_count, 2);
        assert_eq!(m.prefix, "App\\Models");
    }

    #[test]
    fn finds_member_in_multiline_group() {
        let decl = "use App\\Models\\{\n    User,\n    Post,\n}";
        let m = find_use_member(decl, "App\\Models\\User", "User").unwrap();
        assert_eq!(&decl[m.start..m.end], "User");
        assert_eq!(m.member_count, 2, "a trailing comma does not add a member");
    }

    #[test]
    fn group_member_span_covers_alias_clause() {
        let decl = "use App\\Models\\{User, Post as BlogPost}";
        let m = find_use_member(decl, "App\\Models\\Post", "BlogPost").unwrap();
        assert_eq!(&decl[m.start..m.end], "Post as BlogPost");
    }

    #[test]
    fn group_member_matches_nested_name() {
        let decl = "use App\\{Models\\User, Post}";
        let m = find_use_member(decl, "App\\Models\\User", "User").unwrap();
        assert_eq!(&decl[m.start..m.end], "Models\\User");
    }

    #[test]
    fn group_member_matches_per_member_modifier() {
        let decl = "use App\\{function helper, const LIMIT}";
        let m = find_use_member(decl, "App\\LIMIT", "LIMIT").unwrap();
        assert_eq!(&decl[m.start..m.end], "const LIMIT");
    }

    #[test]
    fn group_member_does_not_match_prefix_sharing_name() {
        // `App\Models\SuperUser` must not be matched when looking for
        // `App\Models\User`.
        let decl = "use App\\Models\\{SuperUser}";
        assert!(find_use_member(decl, "App\\Models\\User", "User").is_none());
    }

    #[test]
    fn group_member_requires_matching_alias() {
        let decl = "use App\\Models\\{Post as BlogPost}";
        assert!(find_use_member(decl, "App\\Models\\Post", "Post").is_none());
    }

    #[test]
    fn group_member_after_a_comment_is_still_found() {
        let decl = "use Uses\\{\n\tBar,\n\tNonexistent, // could be namespace\n\tfunction Foo as fooAgain,\n\tconst MY_CONSTANT\n}";
        let m = find_use_member(decl, "Uses\\Foo", "fooAgain").unwrap();
        assert_eq!(&decl[m.start..m.end], "function Foo as fooAgain");
        assert_eq!(m.member_count, 4);
    }

    #[test]
    fn group_member_trailing_comment_is_not_part_of_the_name() {
        let decl = "use Uses\\{\n\tBar, // keep\n\tBaz\n}";
        let m = find_use_member(decl, "Uses\\Bar", "Bar").unwrap();
        assert_eq!(&decl[m.start..m.end], "Bar");
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
        assert!(find_use_member("use App\\Foo", "App\\Foo", "Foo").is_some());
        assert!(find_use_member("use App\\FooBar", "App\\Foo", "Foo").is_none());
    }

    #[test]
    fn brace_less_comma_list_matches_one_item_at_a_time() {
        let decl = "use App\\Models\\User, App\\Models\\Post";
        let m = find_use_member(decl, "App\\Models\\Post", "Post").unwrap();
        assert_eq!(&decl[m.start..m.end], "App\\Models\\Post");
        assert_eq!(m.member_count, 2);
        assert_eq!(m.prefix, "");
    }
}
