//! Where a Blade template's imports go.
//!
//! A template imports with the `@use` directive rather than a PHP `use`
//! statement, and its directives live in the template's own text rather
//! than in the virtual PHP the preprocessor lowers it to, which hoists them
//! into its prologue.  The block a new import joins is therefore read from
//! the template with the scanner in [`super::use_directive`], and the
//! import is written in that syntax.

use tower_lsp::lsp_types::{Position, Range, TextEdit};

use super::directives::{DirectiveHead, directive_head};
use super::source_map::BladeSourceMap;
use super::use_directive::{first_string_literal, imported_name, use_directives};
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
    /// Where an import that precedes every existing one goes.
    top: TemplateTop,
}

/// Where a template takes an import that precedes every `@use` it has.
///
/// Blade compiles `@use` to a PHP `use` statement where it stands, and PHP
/// rejects one inside a block, so the import has to stay out of whatever
/// the template's first line opens.
#[derive(Debug, Clone, Copy)]
enum TemplateTop {
    /// The start of the template, which the import is written before.
    LineStart(Position),
    /// Just inside the `@php` / `<?php` block the template opens with,
    /// where the import is a PHP `use` statement rather than a directive.
    PhpBlock(Position),
    /// The end of the line on which the directive the template opens with
    /// closes: a directive that lowers to nothing at all (`@use`,
    /// `@inject`) leaves no virtual column for the template's start, and
    /// opens no block for the import to land in.
    LineEnd(Position),
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
        let directive = || match alias {
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

        let (position, new_text) = match (anchor, self.top) {
            (Some(end), _) => (end, format!("\n{}", directive())),
            (None, TemplateTop::LineStart(start)) => (start, format!("{}\n", directive())),
            (None, TemplateTop::PhpBlock(inside)) => (
                inside,
                match alias {
                    Some(alias) => format!("\nuse {import} as {alias};"),
                    None => format!("\nuse {import};"),
                },
            ),
            (None, TemplateTop::LineEnd(end)) => (end, format!("\n{}", directive())),
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

    for directive in use_directives(template) {
        let Some((_, literal)) = first_string_literal(directive.arguments) else {
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
        let end = to_php(line_end_at(directive.span.end));
        existing.push((end.line, sort_key));
        line_ends.push(end);
    }

    // The start of the virtual line the template's first line lowers to.
    // A Blade position at the start of a token maps to the end of the PHP
    // the token lowers to, but the start of the line maps back to the
    // template's start, since that is where the generated PHP begins.  Only
    // a leading construct that lowers to nothing at all leaves no column
    // there, and the trip back lands past it.
    let start = Position {
        line: 0,
        character: 0,
    };
    let line_start = Position {
        character: 0,
        ..to_php(start)
    };
    let top = match map.map_or(Some(start), |map| map.try_php_to_blade(line_start)) {
        Some(lands_at) if lands_at == start => TemplateTop::LineStart(line_start),
        Some(lands_at) if opens_php_block(template, lands_at.character) => {
            TemplateTop::PhpBlock(line_start)
        }
        _ => TemplateTop::LineEnd(to_php(line_end_at(leading_directive_end(template)))),
    };

    UseBlockInfo {
        existing,
        fallback_line: line_start.line,
        has_namespace: false,
        template: Some(TemplateUseBlock { line_ends, top }),
    }
}

/// Whether the template's first `columns` columns, which lowered to
/// nothing, open a block of PHP: `@php`, or a raw `<?php` / `<?` tag.
fn opens_php_block(template: &str, columns: u32) -> bool {
    match template.get(..columns as usize) {
        Some("@php" | "<?php") => true,
        // A short open tag, rather than a longer word starting with one.
        Some("<?") => template[2..].starts_with(char::is_whitespace),
        _ => false,
    }
}

/// The byte offset at which the directive the template opens with ends:
/// past its argument list when it has one, which may run over several
/// lines, and the template's start otherwise.
fn leading_directive_end(template: &str) -> usize {
    let bytes = template.as_bytes();
    match directive_head(template, bytes, 0, bytes.len()) {
        DirectiveHead::Named {
            args: Some(args), ..
        } => args.end,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blade::TemplateKind;
    use crate::blade::preprocessor::{
        ComponentBinding, ComponentResolver, ComponentTarget, preprocess, preprocess_with_vars,
    };
    use crate::completion::use_edit::{
        build_aliased_use_edit, build_use_edit, build_use_function_edit,
    };

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

    /// Plan an import of `fqn` against the lowering `map` describes, move
    /// the edit back through the map the way `src/blade/translate.rs` does,
    /// and apply it to the template.
    fn import_through(template: &str, map: &BladeSourceMap, fqn: &str) -> String {
        let info = analyze_template_use_block(template, Some(map));
        let mut edits = build_use_edit(fqn, &info, &None).expect("an import is needed");
        for edit in &mut edits {
            let back = |position| {
                map.try_php_to_blade(position)
                    .expect("the edit must map back into the template")
            };
            edit.range = Range {
                start: back(edit.range.start),
                end: back(edit.range.end),
            };
        }
        apply(template, &edits)
    }

    fn import_into(template: &str, fqn: &str) -> String {
        import_through(template, &preprocess(template).1, fqn)
    }

    /// Blade compiles `@use` to a PHP `use` statement where it stands, and
    /// PHP rejects one inside the block a first line opens, so the import
    /// goes above that line rather than after it.
    #[test]
    fn a_first_import_goes_above_the_block_the_first_line_opens() {
        assert_eq!(
            import_into(
                "@if ($user)\n    {{ Carbon::now() }}\n@endif\n",
                "Carbon\\Carbon"
            ),
            "@use('Carbon\\Carbon')\n@if ($user)\n    {{ Carbon::now() }}\n@endif\n"
        );
    }

    /// Every opener that lowers to PHP of its own starts the virtual line
    /// with that PHP, so the line's start maps back to the template's.
    #[test]
    fn a_first_line_that_lowers_to_php_takes_the_import_above_it() {
        for first_line in [
            "{{ $title }}",
            "{!! $title !!}",
            "{{-- a note --}}",
            "@extends('layouts.app')",
            "@php($title = 'Home')",
            "<?= $title ?>",
            "<x-unknown-tag />",
        ] {
            let template = format!("{first_line}\n<p>{{{{ $body }}}}</p>\n");
            assert_eq!(
                import_into(&template, "App\\Models\\Widget"),
                format!("@use('App\\Models\\Widget')\n{template}"),
                "a template opening with {first_line:?}"
            );
        }
    }

    /// A component tag the project resolves, whose call is emitted where
    /// the tag closes, still keeps the start of its line addressable, so
    /// the import goes above the tag rather than into its slot.
    #[test]
    fn a_first_import_goes_above_a_resolved_component_tag() {
        struct Alert;
        impl ComponentResolver for Alert {
            fn x_component(&self, _: &str) -> Option<ComponentTarget> {
                Some(ComponentTarget {
                    fqn: "App\\View\\Components\\Alert".to_string(),
                    binding: ComponentBinding::Construct(Vec::new()),
                })
            }
            fn livewire_component(&self, _: &str) -> Option<ComponentTarget> {
                None
            }
        }

        let template = "<x-alert type=\"danger\">\n    {{ $message }}\n</x-alert>\n";
        let (_, map) = preprocess_with_vars(
            template,
            &[],
            TemplateKind::View,
            None,
            Some(&Alert),
            &Default::default(),
        );
        assert_eq!(
            import_through(template, &map, "App\\Models\\Widget"),
            format!("@use('App\\Models\\Widget')\n{template}")
        );
    }

    /// A `@use` inside a verbatim block is text Blade prints, not an
    /// import, so a template opening with one takes the import above it.
    #[test]
    fn a_first_import_goes_above_a_verbatim_block() {
        let template = "@verbatim\n<div>{{ message }}</div>\n@endverbatim\n<p>{{ $x }}</p>\n";
        assert_eq!(
            import_into(template, "App\\Models\\Widget"),
            format!("@use('App\\Models\\Widget')\n{template}")
        );
    }

    /// A template that opens with a block of PHP takes the import as a
    /// PHP `use` statement just inside that block.
    #[test]
    fn a_template_opening_with_a_php_block_takes_a_use_statement_inside_it() {
        for (template, imported) in [
            (
                "@php\n$total = 0;\n@endphp\n",
                "@php\nuse App\\Models\\Widget;\n$total = 0;\n@endphp\n",
            ),
            (
                "<?php\n$total = 0;\n?>\n",
                "<?php\nuse App\\Models\\Widget;\n$total = 0;\n?>\n",
            ),
            (
                "@php $total = 0; @endphp\n",
                "@php\nuse App\\Models\\Widget; $total = 0; @endphp\n",
            ),
        ] {
            assert_eq!(import_into(template, "App\\Models\\Widget"), imported);
        }

        let template = "@php\n$total = 0;\n@endphp\n";
        let (_, map) = preprocess(template);
        let info = analyze_template_use_block(template, Some(&map));
        let texts = [
            build_aliased_use_edit("App\\Models\\Widget", Some("Gadget"), &info, &None),
            build_use_function_edit("App\\Support\\format_price", &info),
        ]
        .map(|edits| edits.expect("an import is needed")[0].new_text.clone());
        assert_eq!(
            texts,
            [
                "\nuse App\\Models\\Widget as Gadget;",
                "\nuse function App\\Support\\format_price;"
            ]
        );
    }

    /// A leading directive that lowers to nothing at all leaves the
    /// template's start unaddressable, and opens no block, so the import
    /// follows the line the directive closes on.
    #[test]
    fn a_first_import_follows_a_leading_directive_that_lowers_to_nothing() {
        assert_eq!(
            import_into(
                "@inject('metrics', 'App\\Metrics')\n<p>{{ $x }}</p>\n",
                "App\\Models\\Widget"
            ),
            "@inject('metrics', 'App\\Metrics')\n@use('App\\Models\\Widget')\n<p>{{ $x }}</p>\n"
        );
        assert_eq!(
            import_into(
                "@inject(\n    'metrics',\n    'App\\Metrics'\n)\n<p>{{ $x }}</p>\n",
                "App\\Models\\Widget"
            ),
            "@inject(\n    'metrics',\n    'App\\Metrics'\n)\n@use('App\\Models\\Widget')\n\
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
