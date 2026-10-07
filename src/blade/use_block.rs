//! Where a Blade template's imports go.
//!
//! A template imports with the `@use` directive rather than a PHP `use`
//! statement, and its directives live in the template's own text rather
//! than in the virtual PHP the preprocessor lowers it to, which hoists them
//! into its prologue.  The block a new import joins is therefore read from
//! the template with the scanner in [`super::use_directive`], and the
//! import is written in that syntax.

use tower_lsp::lsp_types::{Position, Range, TextEdit};

use super::source_map::BladeSourceMap;
use super::use_directive::{first_string_literal, imported_name, use_directive_arguments};
use crate::completion::use_edit::{UseBlockInfo, extract_use_sort_key};

/// Where a Blade template's imports go, in the virtual-PHP coordinates
/// every edit is planned in.
///
/// A template imports with `@use('App\Models\Widget')` on a line of its
/// own, and a new one is anchored at the **end** of the line it follows:
/// the directive lowers to nothing, so the start of its line shares a
/// virtual column with the text that comes after it and the trip back
/// through the source map cannot tell the two apart.  The end of a line
/// is unambiguous in both directions.
#[derive(Debug, Clone)]
pub(crate) struct TemplateUseBlock {
    /// The end of the line each existing `@use` sits on, in the order
    /// [`UseBlockInfo::existing`] lists them.
    line_ends: Vec<Position>,
    /// Where an import that precedes every existing one goes: the start of
    /// the template, or the end of its first line when the template opens
    /// with a directive of its own, whose line start the virtual PHP
    /// cannot address.
    top: Position,
    /// Whether `top` is the start of a line, so the import is written
    /// before what stands there rather than after it.
    top_at_line_start: bool,
}

impl TemplateUseBlock {
    /// The edit that imports `import` (a name, or a name behind its
    /// `function` / `const` modifier) as `@use('import')`, or under `alias`
    /// as `@use('import', 'alias')`.
    ///
    /// `key` is the import's sort key, compared against the `existing`
    /// imports' keys ([`UseBlockInfo::existing`]).  The import follows the
    /// last directive it sorts behind, written at the end of that
    /// directive's line, and goes to the top of the template when it sorts
    /// before all of them.  Anchoring on the line that comes *before* the
    /// new import rather than the one that comes after is what keeps the
    /// position addressable in the virtual PHP.
    pub(crate) fn import_edit(
        &self,
        existing: &[(u32, String)],
        key: &str,
        import: &str,
        alias: Option<&str>,
    ) -> TextEdit {
        let statement = match alias {
            Some(alias) => format!("@use('{import}', '{alias}')"),
            None => format!("@use('{import}')"),
        };

        let comes_before = |existing: &str| {
            (UseBlockInfo::key_group(existing), existing) < (UseBlockInfo::key_group(key), key)
        };
        let anchor = existing
            .iter()
            .zip(&self.line_ends)
            .filter(|((_, existing), _)| comes_before(existing))
            .map(|(_, end)| *end)
            .next_back();

        let (position, new_text) = match (anchor, self.top_at_line_start) {
            (Some(end), _) => (end, format!("\n{statement}")),
            (None, true) => (self.top, format!("{statement}\n")),
            (None, false) => (self.top, format!("\n{statement}")),
        };
        TextEdit {
            range: Range {
                start: position,
                end: position,
            },
            new_text,
        }
    }
}

/// [`crate::completion::use_edit::analyze_use_block`] for a Blade
/// template, read from the template's own text.
///
/// A template imports with `@use('App\Models\Widget')`, which the
/// preprocessor hoists into the virtual PHP's prologue as a real `use`
/// statement.  Scanning the virtual PHP would therefore place a new import
/// in the prologue, which no template text stands behind, so the
/// directives are read from the template with the scanner in
/// [`super::use_directive`] instead.
///
/// The positions recorded are the *virtual PHP* ones the template's own
/// lines lower to, because that is the coordinate system every feature
/// plans its edits in; `src/blade/translate.rs` moves the finished edit
/// back into the template.  `map` is the template's source map, or `None`
/// for a template that was never lowered, whose own coordinates are what
/// the untranslated edit already names.
pub(crate) fn analyze_template_use_block(
    template: &str,
    map: Option<&BladeSourceMap>,
) -> UseBlockInfo {
    let to_php = |position: Position| match map {
        Some(map) => map.blade_to_php(position),
        None => position,
    };

    // The end of the line `at` falls on, in the template's coordinates.
    let line_end_at = |at: usize| {
        let mut end = template[at..]
            .find('\n')
            .map_or(template.len(), |offset| at + offset);
        if template[..end].ends_with('\r') {
            end -= 1;
        }
        crate::text_position::offset_to_position(template, end)
    };

    let mut existing: Vec<(u32, String)> = Vec::new();
    let mut line_ends: Vec<Position> = Vec::new();

    for (arguments_at, arguments) in use_directive_arguments(template) {
        let Some((_, literal)) = first_string_literal(arguments) else {
            continue;
        };
        if imported_name(literal).is_none() {
            continue;
        }
        // The literal is a `use` statement's body written as a string, so
        // the scanner for a PHP import's sort key answers for it verbatim.
        let Some(sort_key) = extract_use_sort_key(&format!("use {};", literal.trim())) else {
            continue;
        };

        // The end of the line the directive closes on, so a multi-line
        // argument list is followed rather than split.
        let end = to_php(line_end_at(arguments_at + arguments.len()));
        existing.push((end.line, sort_key));
        line_ends.push(end);
    }

    // A template that opens with a directive of its own lowers its first
    // columns to nothing, leaving the start of the line sharing a virtual
    // column with the text after it; the trip back answers the latter, so
    // the end of that line is what the import is written after instead.
    let start = Position {
        line: 0,
        character: 0,
    };
    let top = to_php(start);
    let top_at_line_start = map.is_none_or(|map| map.try_php_to_blade(top) == Some(start));

    UseBlockInfo {
        existing,
        fallback_line: top.line,
        has_namespace: false,
        template: Some(TemplateUseBlock {
            line_ends,
            top: if top_at_line_start {
                top
            } else {
                to_php(line_end_at(0))
            },
            top_at_line_start,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::completion::use_edit::{build_use_edit, build_use_function_edit};

    /// Apply the edits to the template they were planned against, which is
    /// what the editor does once the source map has moved them back.
    fn apply(template: &str, edits: &[TextEdit]) -> String {
        let offset = |position: Position| {
            template
                .split_inclusive('\n')
                .take(position.line as usize)
                .map(str::len)
                .sum::<usize>()
                + position.character as usize
        };
        let mut result = template.to_string();
        for edit in edits {
            result.replace_range(
                offset(edit.range.start)..offset(edit.range.end),
                &edit.new_text,
            );
        }
        result
    }

    /// A template with nothing to sort against takes the import at its top,
    /// written as the directive Blade imports with.
    #[test]
    fn a_templates_first_import_goes_to_the_top_as_a_use_directive() {
        let template = "<h1>{{ $title }}</h1>\n";
        let info = analyze_template_use_block(template, None);
        let edits =
            build_use_edit("App\\Models\\Widget", &info, &None).expect("an import is needed");

        assert_eq!(
            apply(template, &edits),
            "@use('App\\Models\\Widget')\n<h1>{{ $title }}</h1>\n"
        );
    }

    /// The directives already in the template are the block the new one
    /// joins, and it is written at the end of the last line it sorts behind.
    #[test]
    fn a_template_import_sorts_among_the_directives_it_already_has() {
        let template = "@use('App\\Models\\Account')\n@use('App\\Models\\Zone')\n<p>{{ $x }}</p>\n";
        let info = analyze_template_use_block(template, None);

        let between =
            build_use_edit("App\\Models\\Widget", &info, &None).expect("an import is needed");
        assert_eq!(
            apply(template, &between),
            "@use('App\\Models\\Account')\n@use('App\\Models\\Widget')\n\
             @use('App\\Models\\Zone')\n<p>{{ $x }}</p>\n"
        );

        let after = build_use_edit("App\\Models\\Zulu", &info, &None).expect("an import is needed");
        assert_eq!(
            apply(template, &after),
            "@use('App\\Models\\Account')\n@use('App\\Models\\Zone')\n\
             @use('App\\Models\\Zulu')\n<p>{{ $x }}</p>\n"
        );

        let before =
            build_use_edit("App\\Models\\Aardvark", &info, &None).expect("an import is needed");
        assert_eq!(
            apply(template, &before),
            "@use('App\\Models\\Aardvark')\n@use('App\\Models\\Account')\n\
             @use('App\\Models\\Zone')\n<p>{{ $x }}</p>\n"
        );
    }

    /// Every import of a batch starts on a line of its own: the line break
    /// a template import opens with is not the blank line a PHP batch drops
    /// from all but its first import.
    #[test]
    fn a_batch_of_template_imports_keeps_their_line_breaks() {
        let template = "@use('App\\Models\\Account')\n<p>{{ $x }}</p>\n";
        let info = analyze_template_use_block(template, None);
        let mut batch = Vec::new();
        for fqn in ["App\\Models\\Widget", "App\\Models\\Zone"] {
            let mut edits = build_use_edit(fqn, &info, &None).expect("an import is needed");
            if !batch.is_empty() {
                info.drop_repeated_separator(&mut edits);
            }
            batch.extend(edits);
        }
        let texts: Vec<&str> = batch.iter().map(|edit| edit.new_text.as_str()).collect();
        assert_eq!(
            texts,
            [
                "\n@use('App\\Models\\Widget')",
                "\n@use('App\\Models\\Zone')"
            ]
        );
    }

    /// A function import keeps its modifier inside the directive's literal,
    /// which is how Blade spells one, and still sorts after the classes.
    #[test]
    fn a_template_function_import_keeps_the_modifier_in_the_literal() {
        let template = "@use('App\\Models\\Widget')\n<p>{{ $x }}</p>\n";
        let info = analyze_template_use_block(template, None);
        let edits = build_use_function_edit("App\\Support\\format_price", &info)
            .expect("a namespaced function needs an import");

        assert_eq!(
            apply(template, &edits),
            "@use('App\\Models\\Widget')\n@use('function App\\Support\\format_price')\n\
             <p>{{ $x }}</p>\n"
        );
    }

    /// The modifier and the alias forms a template may already have are read
    /// as the imports they are, so an equal name is not imported twice.
    #[test]
    fn the_directive_forms_a_template_already_has_are_read_as_imports() {
        let template =
            "@use('App\\Models\\Widget as Gadget')\n@use('function App\\Support\\helper')\n";
        let info = analyze_template_use_block(template, None);

        assert_eq!(
            info.existing,
            vec![
                (0, "app\\models\\widget".to_string()),
                (1, "function app\\support\\helper".to_string()),
            ]
        );
        assert!(
            build_use_function_edit("App\\Support\\helper", &info).is_none(),
            "the function is already imported"
        );
    }
}
