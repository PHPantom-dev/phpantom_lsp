use super::*;

fn names(vars: &[(String, PhpType)]) -> Vec<&str> {
    vars.iter().map(|(n, _)| n.as_str()).collect()
}

/// A project whose tags name no component the preprocessor could
/// build, so no attribute of theirs is an argument.
fn no_arguments(_tag: &str) -> Option<Vec<String>> {
    None
}

#[test]
fn referenced_component_tags_collects_distinct_names() {
    let content = r#"<x-brand.boxes /><x-brand.boxes /><x-alert type="danger" />"#;
    let mut tags = referenced_component_tags(content);
    tags.sort();
    assert_eq!(tags, vec!["alert", "brand.boxes"]);
}

#[test]
fn scans_a_literal_string_attribute() {
    let calls = scan_component_tag_calls(
        r#"<x-alert type="danger" />"#,
        &["alert".to_string()],
        &no_arguments,
    );
    assert_eq!(calls.len(), 1);
    assert_eq!(names(&calls[0].literal), vec!["type"]);
    assert_eq!(
        calls[0].literal[0].1,
        PhpType::literal_string_value("danger")
    );
}

#[test]
fn scans_a_bare_boolean_attribute() {
    let calls = scan_component_tag_calls(
        r#"<x-alert disabled />"#,
        &["alert".to_string()],
        &no_arguments,
    );
    assert_eq!(
        calls[0].literal,
        vec![("disabled".to_string(), PhpType::bool())]
    );
}

#[test]
fn camel_cases_a_hyphenated_attribute_name() {
    let calls = scan_component_tag_calls(
        r#"<x-alert hair-analysis="x" />"#,
        &["alert".to_string()],
        &no_arguments,
    );
    assert_eq!(names(&calls[0].literal), vec!["hairAnalysis"]);
}

#[test]
fn a_bound_attribute_is_indexed_in_document_order() {
    let calls = scan_component_tag_calls(
        r#"<x-alert :hairAnalysis="$model->hairAnalysis" />"#,
        &["alert".to_string()],
        &no_arguments,
    );
    assert_eq!(calls[0].bound, vec![("hairAnalysis".to_string(), 0)]);
}

#[test]
fn a_non_matching_tags_bound_attribute_still_advances_the_index() {
    let calls = scan_component_tag_calls(
        r#"<div :class="$active"></div><x-alert :message="$msg" />"#,
        &["alert".to_string()],
        &no_arguments,
    );
    // The `<div>` binding is index 0; `alert`'s own binding must
    // therefore be index 1, or the caller correlates it against the
    // wrong `blade_bound_attr_directive` call.
    assert_eq!(calls[0].bound, vec![("message".to_string(), 1)]);
}

#[test]
fn a_shorthand_bound_attribute_is_named_after_its_variable() {
    let calls = scan_component_tag_calls(
        r#"<x-alert :$message />"#,
        &["alert".to_string()],
        &no_arguments,
    );
    assert_eq!(calls[0].bound, vec![("message".to_string(), 0)]);
}

#[test]
fn a_non_matching_tag_contributes_nothing() {
    let calls = scan_component_tag_calls(
        r#"<x-widget foo="bar" />"#,
        &["alert".to_string()],
        &no_arguments,
    );
    assert!(calls.is_empty());
}

#[test]
fn an_echo_interpolated_literal_falls_back_to_a_generic_string() {
    let calls = scan_component_tag_calls(
        r#"<x-alert title="Hello {{ $name }}" />"#,
        &["alert".to_string()],
        &no_arguments,
    );
    assert_eq!(calls[0].literal[0].1, PhpType::string());
}

/// A utility class carrying an arbitrary value (`max-h-[80vh]`) puts
/// brackets inside a quoted attribute value. The scan tracks quotes, so
/// neither the attribute nor the rest of the tag is cut short by them.
#[test]
fn a_bracket_in_a_quoted_attribute_value_does_not_truncate_the_tag() {
    let calls = scan_component_tag_calls(
        r#"<x-modal class="max-h-[80vh]" title="Save" /><x-alert type="danger" />"#,
        &["modal".to_string(), "alert".to_string()],
        &no_arguments,
    );
    assert_eq!(calls.len(), 2, "both tags must be seen");
    assert_eq!(names(&calls[0].literal), vec!["class", "title"]);
    assert_eq!(
        calls[0].literal[0].1,
        PhpType::literal_string_value("max-h-[80vh]")
    );
    assert_eq!(names(&calls[1].literal), vec!["type"]);
}

#[test]
fn a_component_tag_inside_a_comment_is_ignored() {
    let calls = scan_component_tag_calls(
        r#"{{-- <x-alert type="danger" /> --}}"#,
        &["alert".to_string()],
        &no_arguments,
    );
    assert!(calls.is_empty());
}

#[test]
fn scans_an_inline_named_slot() {
    let names = scan_component_tag_slots(
        "<x-card><x-slot:title>Hello</x-slot></x-card>",
        &["card".to_string()],
    );
    assert_eq!(names, vec!["title"]);
}

#[test]
fn scans_the_legacy_name_attribute_form() {
    let names = scan_component_tag_slots(
        r#"<x-card><x-slot name="title">Hello</x-slot></x-card>"#,
        &["card".to_string()],
    );
    assert_eq!(names, vec!["title"]);
}

#[test]
fn camel_cases_a_hyphenated_inline_slot_name() {
    let names = scan_component_tag_slots(
        "<x-card><x-slot:hair-analysis>x</x-slot></x-card>",
        &["card".to_string()],
    );
    assert_eq!(names, vec!["hairAnalysis"]);
}

/// Blade's own `Str::camel` transform only fires on the inline form:
/// a hyphenated `name="…"` attribute is used verbatim, which is not a
/// legal PHP variable name and so is never actually bound.
#[test]
fn a_hyphenated_legacy_name_is_not_camel_cased() {
    let names = scan_component_tag_slots(
        r#"<x-card><x-slot name="hair-analysis">x</x-slot></x-card>"#,
        &["card".to_string()],
    );
    assert_eq!(names, vec!["hair-analysis"]);
}

/// A plain HTML tag between the component and its slot does not
/// change which component receives the slot.
#[test]
fn html_nesting_does_not_block_slot_scoping() {
    let names = scan_component_tag_slots(
        "<x-card><div><x-slot:title>Hello</x-slot></div></x-card>",
        &["card".to_string()],
    );
    assert_eq!(names, vec!["title"]);
}

/// A slot inside a *different* nested component belongs to that
/// component, not the outer one.
#[test]
fn a_nested_components_own_slot_is_not_the_outer_ones() {
    let names = scan_component_tag_slots(
        "<x-card><x-alert><x-slot:title>Hello</x-slot></x-alert></x-card>",
        &["card".to_string()],
    );
    assert!(names.is_empty());

    let names = scan_component_tag_slots(
        "<x-card><x-alert><x-slot:title>Hello</x-slot></x-alert></x-card>",
        &["alert".to_string()],
    );
    assert_eq!(names, vec!["title"]);
}

#[test]
fn the_same_slot_name_is_reported_once() {
    let names = scan_component_tag_slots(
        "<x-card><x-slot:title>A</x-slot></x-card><x-card><x-slot:title>B</x-slot></x-card>",
        &["card".to_string()],
    );
    assert_eq!(names, vec!["title"]);
}

#[test]
fn a_slot_outside_any_matching_tag_contributes_nothing() {
    let names = scan_component_tag_slots("<x-slot:title>Hello</x-slot>", &["card".to_string()]);
    assert!(names.is_empty());
}

/// The lexed attributes as `(name, bound, shorthand, value)` for
/// readable assertions.
fn lexed(tag: &str) -> Vec<(&str, bool, bool, Option<&str>)> {
    lex_tag_attributes(tag, 0)
        .attributes
        .into_iter()
        .map(|attr| {
            (
                &tag[attr.name],
                attr.bound,
                attr.shorthand,
                attr.value.map(|value| &tag[value]),
            )
        })
        .collect()
}

#[test]
fn lexes_every_attribute_shape() {
    assert_eq!(
        lexed(r#" type="info" disabled :items="$a > $b" :$user data-x=1 ::class="a">"#),
        [
            ("type", false, false, Some("info")),
            ("disabled", false, false, None),
            ("items", true, false, Some("$a > $b")),
            ("user", true, true, None),
            ("data-x", false, false, Some("1")),
            (":class", false, false, Some("a")),
        ]
    );
}

#[test]
fn the_tag_ends_at_the_first_close_outside_a_value() {
    let tag = r#" :when="$a > $b" /> tail"#;
    let lexed = lex_tag_attributes(tag, 0);
    assert!(lexed.closed);
    assert!(lexed.self_closing);
    assert_eq!(&tag[lexed.end..], " tail");
}

#[test]
fn a_value_that_never_closes_leaves_the_tag_unclosed() {
    let lexed = lex_tag_attributes(r#" title="oops>"#, 0);
    assert!(!lexed.closed);
    assert_eq!(lexed.attributes.len(), 1);
}

#[test]
fn kebab_matches_laravels_own_spelling() {
    assert_eq!(kebab_case("DatePicker"), "date-picker");
    assert_eq!(kebab_case("Alert"), "alert");
    assert_eq!(kebab_case("HTMLPurifier"), "h-t-m-l-purifier");
    assert_eq!(kebab_case("Create_Refund"), "create_-refund");
}

/// The cursor's context is taken at the `|` marker, which is stripped
/// before the scan so the surrounding text is what the user typed.
fn context_at(marked: &str) -> Option<TagContext> {
    let offset = marked.find('|').expect("no cursor marker");
    tag_context_at(&marked.replace('|', ""), offset)
}

fn name_context(marked: &str) -> Option<(TagKind, String, String)> {
    let offset = marked.find('|').expect("no cursor marker");
    let content = marked.replace('|', "");
    let ctx = tag_context_at(&content, offset)?;
    (ctx.cursor == TagCursor::Name).then(|| {
        (
            ctx.kind,
            ctx.name.clone(),
            content[ctx.token_start..offset].to_string(),
        )
    })
}

#[test]
fn a_bare_opening_is_a_name_with_nothing_typed() {
    assert_eq!(
        name_context("<div><x-|"),
        Some((TagKind::Blade, String::new(), String::new()))
    );
}

#[test]
fn a_partly_typed_name_carries_what_is_typed_so_far() {
    assert_eq!(
        name_context("<x-for|ms.input>"),
        Some((TagKind::Blade, "forms.input".to_string(), "for".to_string()))
    );
}

#[test]
fn a_livewire_opening_is_its_own_kind() {
    assert_eq!(
        name_context("<livewire:coun|"),
        Some((TagKind::Livewire, "coun".to_string(), "coun".to_string()))
    );
}

#[test]
fn a_cursor_after_the_tag_name_is_at_an_attribute() {
    let ctx = context_at("<x-alert |").expect("expected an attribute context");
    assert_eq!(ctx.cursor, TagCursor::Attribute);
    assert_eq!(ctx.name, "alert");
    assert_eq!(ctx.token_start, "<x-alert ".len());
}

#[test]
fn a_partly_typed_attribute_starts_at_its_own_first_character() {
    let ctx = context_at(r#"<x-alert class="a" :mes|"#).expect("expected an attribute");
    assert_eq!(ctx.cursor, TagCursor::Attribute);
    assert_eq!(ctx.token_start, r#"<x-alert class="a" "#.len());
}

#[test]
fn a_closed_tag_leaves_the_cursor_outside_it() {
    assert_eq!(context_at("<x-alert /> |"), None);
    assert_eq!(context_at("<x-alert>{{ $com|"), None);
}

/// An attribute value may hold a `>` (`:items=\"$a > $b\"`), so the
/// scan has to read quotes rather than stop at the first one.
#[test]
fn a_greater_than_inside_a_value_does_not_close_the_tag() {
    let ctx = context_at(r#"<x-alert :items="$a > $b" |"#).expect("expected an attribute");
    assert_eq!(ctx.cursor, TagCursor::Attribute);
}

#[test]
fn a_cursor_inside_an_attribute_value_is_not_writing_an_attribute() {
    assert_eq!(context_at(r#"<x-alert type="dan|"#), None);
    assert_eq!(context_at("<x-alert type=dan|"), None);
}

#[test]
fn a_tag_written_inside_a_comment_names_nothing() {
    assert_eq!(context_at("{{-- <x-al| --}}"), None);
}

#[test]
fn a_closing_tag_is_not_an_opening_one() {
    assert_eq!(context_at("<div></x-al|"), None);
}

/// The spans of the tags [`tag_spans`] found a body for, as
/// `(start, end)` pairs, for readable assertions.
fn tag_bodies(content: &str) -> Vec<(usize, usize)> {
    tag_spans(content)
        .into_iter()
        .filter(|tag| tag.closed)
        .map(|tag| (tag.span.start, tag.span.end))
        .collect()
}

#[test]
fn a_component_tag_body_runs_from_open_to_close() {
    let blade = "<x-alert>\n<p>hi</p>\n</x-alert>\n";
    assert_eq!(tag_bodies(blade), [(0, blade.len() - 1)]);
}

#[test]
fn a_self_closing_tag_has_no_body() {
    let blade = "<x-alert />\n<p>after</p>\n";
    assert!(tag_bodies(blade).is_empty());
    // It is still a tag, spanning itself alone.
    let tags = tag_spans(blade);
    assert_eq!(tags.len(), 1);
    assert_eq!(&blade[tags[0].span.clone()], "<x-alert />");
}

#[test]
fn nested_component_tags_each_span_independently() {
    let blade = "<x-card>\n<x-alert>\n<p>hi</p>\n</x-alert>\n</x-card>\n";
    assert_eq!(tag_bodies(blade).len(), 2);
}

#[test]
fn a_mismatched_closing_tag_closes_nothing() {
    assert!(tag_bodies("<x-alert>\n<p>hi</p>\n</x-card>\n").is_empty());
}

/// A stray closing tag ends the innermost open tag, the way Blade
/// compiles it, so the tag's own closer later closes nothing.
#[test]
fn a_stray_closing_tag_ends_the_innermost_tag() {
    let blade = "<x-card>\n<x-alert>\n</x-foo>\n</x-alert>\n</x-card>\n";
    assert!(tag_bodies(blade).is_empty());
    let names: Vec<String> = tag_spans(blade).into_iter().map(|tag| tag.name).collect();
    assert_eq!(names, ["card", "alert"]);
}

#[test]
fn an_unclosed_tag_spans_its_opening_tag_alone() {
    let blade = "<x-alert>\n<p>hi</p>\n";
    assert!(tag_bodies(blade).is_empty());
    let tags = tag_spans(blade);
    assert_eq!(tags.len(), 1);
    assert_eq!(&blade[tags[0].span.clone()], "<x-alert>");
}

#[test]
fn a_livewire_tag_body_is_found() {
    let blade = "<livewire:counter>\n<p>slot</p>\n</livewire:counter>\n";
    assert_eq!(tag_bodies(blade), [(0, blade.len() - 1)]);
    let tags = tag_spans(blade);
    assert_eq!(tags[0].kind, TagKind::Livewire);
    assert_eq!(tags[0].name, "counter");
    assert_eq!(&blade[tags[0].name_span.clone()], "counter");
}

#[test]
fn a_bracket_in_an_attribute_value_does_not_close_the_opening_tag_early() {
    let blade = "<x-alert :items=\"$a > $b\">\n<p>hi</p>\n</x-alert>\n";
    assert_eq!(tag_bodies(blade), [(0, blade.len() - 1)]);
}

#[test]
fn tags_come_back_in_document_order() {
    let blade = "<x-card>\n<x-alert />\n</x-card>\n<x-note />\n";
    let names: Vec<String> = tag_spans(blade).into_iter().map(|tag| tag.name).collect();
    assert_eq!(names, ["card", "alert", "note"]);
}

/// A named inline slot closes with the bare `</x-slot>`, not
/// `</x-slot:title>`; `tag_spans` has to know that too or every named
/// slot in the file comes back unclosed.
#[test]
fn a_named_slot_closes_with_the_bare_tag() {
    let blade = "<x-card>\n<x-slot:title>\nHi\n</x-slot>\n</x-card>\n";
    assert_eq!(tag_bodies(blade).len(), 2);
}

/// The imbalances [`tag_imbalances`] finds, as short strings for
/// readable assertions: `"mismatched </x-card>/<x-alert>"`,
/// `"unexpected </x-card>"`, `"unclosed <x-alert>"`.
fn tag_report(content: &str) -> Vec<String> {
    tag_imbalances(content)
        .into_iter()
        .map(|imbalance| match imbalance {
            TagImbalance::Mismatched { found, opener, .. } => {
                format!("mismatched </x-{found}>/<x-{opener}>")
            }
            TagImbalance::Unexpected { found, .. } => format!("unexpected </x-{found}>"),
            TagImbalance::Unclosed { opener, .. } => format!("unclosed <x-{opener}>"),
        })
        .collect()
}

/// `<x-alert>` closed by `</x-card>` is a mismatched-tag diagnostic:
/// the innermost open tag is what a wrongly-named closer actually
/// ends.
#[test]
fn a_tag_closed_by_another_components_name_is_mismatched() {
    assert_eq!(
        tag_report("<x-alert>\n<p>hi</p>\n</x-card>\n"),
        ["mismatched </x-card>/<x-alert>"]
    );
}

/// `<x-alert>` with no closing tag anywhere is an unclosed-tag
/// diagnostic.
#[test]
fn a_tag_with_no_closing_tag_is_unclosed() {
    assert_eq!(tag_report("<x-alert>\n<p>hi</p>\n"), ["unclosed <x-alert>"]);
}

/// A closing tag with nothing open at all closes nothing.
#[test]
fn a_closing_tag_with_nothing_open_is_unexpected() {
    assert_eq!(
        tag_report("<p>hi</p>\n</x-alert>\n"),
        ["unexpected </x-alert>"]
    );
}

/// Self-closing tags, and tags whose attributes hold a `>`, report
/// nothing.
#[test]
fn self_closing_and_bracket_bearing_tags_report_nothing() {
    assert!(tag_report("<x-alert />\n<p>after</p>\n").is_empty());
    assert!(tag_report("<x-alert :items=\"$a > $b\">\n<p>hi</p>\n</x-alert>\n").is_empty());
}

/// A closer for a tag further out reports only the innermost tag it
/// skipped past, the same as a directive closer that skips open
/// blocks.
#[test]
fn a_closer_matching_a_shallower_tag_reports_what_it_skipped() {
    assert_eq!(
        tag_report("<x-card>\n<x-alert>\n<p>hi</p>\n</x-card>\n"),
        ["mismatched </x-card>/<x-alert>"]
    );
}

/// Properly nested and paired tags report nothing.
#[test]
fn balanced_tags_report_nothing() {
    assert!(
        tag_report(
            "<x-card>\n<x-alert>\n<p>hi</p>\n</x-alert>\n</x-card>\n<livewire:counter></livewire:counter>\n"
        )
        .is_empty()
    );
}

/// A named inline slot that is properly closed reports nothing, even
/// though its closing tag never repeats the slot's own name.
#[test]
fn a_properly_closed_named_slot_reports_nothing() {
    assert!(tag_report("<x-card>\n<x-slot:title>\nHi\n</x-slot>\n</x-card>\n").is_empty());
}

/// A closing tag that does repeat the slot's name is just as valid:
/// Blade's compiler ends the open slot on any `</x-slot…>`, whatever
/// name it carries.
#[test]
fn a_named_slot_closed_under_its_own_name_reports_nothing() {
    assert!(tag_report("<x-card>\n<x-slot:title>\nHi\n</x-slot:title>\n</x-card>\n").is_empty());
    assert!(tag_report("<x-card>\n<x-slot:title>\nHi\n</x-slot:footer>\n</x-card>\n").is_empty());
}
