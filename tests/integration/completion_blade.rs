//! Completion of the parts of a Blade template that are not PHP: the
//! directive name after an `@`, and the component name and attribute names
//! of a `<x-…>` / `<livewire:…>` tag.

use crate::common::{create_psr4_workspace, create_test_backend, open_document};
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

async fn complete_at(
    backend: &phpantom_lsp::Backend,
    uri: &Url,
    line: u32,
    character: u32,
) -> Vec<CompletionItem> {
    let params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position { line, character },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: Some(CompletionContext {
            trigger_kind: CompletionTriggerKind::TRIGGER_CHARACTER,
            trigger_character: Some("@".to_string()),
        }),
    };

    match backend.completion(params).await.unwrap() {
        Some(CompletionResponse::Array(items)) => items,
        Some(CompletionResponse::List(list)) => list.items,
        None => Vec::new(),
    }
}

#[tokio::test]
async fn at_sign_in_html_position_offers_all_known_directives() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///page.blade.php").unwrap();
    open_document(&backend, &uri, "blade", "<div>@</div>").await;

    let items = complete_at(&backend, &uri, 0, 6).await;
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();

    assert!(
        labels.contains(&"@if") && labels.contains(&"@foreach") && labels.contains(&"@endif"),
        "expected the full known-directive list, got: {:?}",
        labels
    );
}

#[tokio::test]
async fn the_if_completion_inserts_the_documented_snippet() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///page.blade.php").unwrap();
    open_document(&backend, &uri, "blade", "<div>@</div>").await;

    let items = complete_at(&backend, &uri, 0, 6).await;
    let if_item = items
        .iter()
        .find(|i| i.label == "@if")
        .expect("expected an @if completion item");

    assert_eq!(
        if_item.insert_text.as_deref(),
        Some("if ($1)\n\t$0\n@endif")
    );
    assert_eq!(if_item.insert_text_format, Some(InsertTextFormat::SNIPPET));
}

#[tokio::test]
async fn a_partial_directive_name_filters_the_list() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///page.blade.php").unwrap();
    open_document(&backend, &uri, "blade", "<div>@for</div>").await;

    // Cursor right after "@for".
    let items = complete_at(&backend, &uri, 0, 9).await;
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();

    assert!(labels.contains(&"@for"), "got: {:?}", labels);
    assert!(labels.contains(&"@foreach"), "got: {:?}", labels);
    assert!(labels.contains(&"@forelse"), "got: {:?}", labels);
    assert!(
        !labels.contains(&"@if"),
        "'@if' does not start with 'for', got: {:?}",
        labels
    );
}

#[tokio::test]
async fn an_unknown_directive_name_still_short_circuits_with_an_empty_list() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///page.blade.php").unwrap();
    // `zzz` matches no directive, but the position is still an HTML/
    // directive-name position, so the strategy must not fall through to
    // (e.g.) class-name completion.
    open_document(&backend, &uri, "blade", "<div>@zzz</div>").await;

    let items = complete_at(&backend, &uri, 0, 9).await;
    assert!(
        items.is_empty(),
        "expected an empty (short-circuited) list, got: {:?}",
        items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn no_directive_completion_inside_echo_braces() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///page.blade.php").unwrap();
    open_document(&backend, &uri, "blade", "{{ @ }}").await;

    // Cursor right after "@" inside `{{ ... }}`.
    let items = complete_at(&backend, &uri, 0, 4).await;
    assert!(
        items.is_empty(),
        "directive completion must not fire inside {{{{ }}}}, got: {:?}",
        items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn no_directive_completion_inside_a_php_block() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///page.blade.php").unwrap();
    open_document(&backend, &uri, "blade", "@php $x = 1; @\n@endphp").await;

    // Cursor right after the trailing "@" on the first line, still inside
    // the `@php ... @endphp` block.
    let items = complete_at(&backend, &uri, 0, 14).await;
    assert!(
        items.is_empty(),
        "directive completion must not fire inside a @php block, got: {:?}",
        items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn directive_completion_still_fires_inside_an_open_block() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///page.blade.php").unwrap();
    // The body between `@if` and `@endif` is ordinary template markup, so
    // a nested directive must still complete.
    open_document(&backend, &uri, "blade", "@if ($x)\n    @\n@endif").await;

    let items = complete_at(&backend, &uri, 1, 5).await;
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(
        labels.contains(&"@foreach"),
        "expected nested directive completion inside @if/@endif, got: {:?}",
        labels
    );
}

// ── Component tag and attribute completion ──────────────────────────────

const COMPOSER: &str = r#"{"autoload": {"psr-4": {
    "App\\": "app/",
    "Illuminate\\": "stubs/Illuminate/",
    "Livewire\\": "stubs/Livewire/"
}}}"#;

const COMPONENT_STUB: &str = "<?php\nnamespace Illuminate\\View;\n\
    abstract class Component {\n\
        public $attributes;\n\
        public function render() {}\n\
    }\n";

const LIVEWIRE_STUB: &str = "<?php\nnamespace Livewire;\n\
    abstract class Component {\n\
        public function render() {}\n\
    }\n";

/// A project with one component of every shape the index answers for: a
/// class-based one, a nested class-based one, an anonymous template, and a
/// Livewire class.
fn component_workspace(template: &str) -> (phpantom_lsp::Backend, tempfile::TempDir, Url) {
    let (backend, dir) = create_psr4_workspace(
        COMPOSER,
        &[
            ("stubs/Illuminate/View/Component.php", COMPONENT_STUB),
            ("stubs/Livewire/Component.php", LIVEWIRE_STUB),
            (
                "app/View/Components/Alert.php",
                "<?php\nnamespace App\\View\\Components;\n\
                 use Illuminate\\View\\Component;\n\
                 class Alert extends Component {\n\
                     public function __construct(\n\
                         public string $type,\n\
                         public ?string $dismissLabel = null,\n\
                     ) {}\n\
                     public function render() {}\n\
                 }\n",
            ),
            (
                "app/View/Components/Forms/Input.php",
                "<?php\nnamespace App\\View\\Components\\Forms;\n\
                 use Illuminate\\View\\Component;\n\
                 class Input extends Component {\n\
                     public function render() {}\n\
                 }\n",
            ),
            (
                "app/Livewire/Counter.php",
                "<?php\nnamespace App\\Livewire;\n\
                 use Livewire\\Component;\n\
                 class Counter extends Component {\n\
                     public int $total = 0;\n\
                     public function mount(int $start): void {}\n\
                     public function render() {}\n\
                 }\n",
            ),
            (
                "resources/views/components/banner.blade.php",
                "@props(['headline', 'subHeadline' => 'none'])\n<div>{{ $headline }}</div>\n",
            ),
            (
                "resources/views/components/hero.blade.php",
                "@aware(['theme'])\n\
                 @php($caption = strtoupper($title))\n\
                 <h1 class=\"{{ $theme }}\">{{ $title }} {{ $subTitle }} {{ $caption }}</h1>\n\
                 @foreach ($rows as $row)\n\
                 <p>{{ $loop->index }}: {{ $row }}</p>\n\
                 @endforeach\n\
                 <div>{{ $slot }}</div>\n",
            ),
            (
                "resources/views/components/notice.blade.php",
                "@props(['level' => 'info'])\n<p class=\"{{ $level }}\">{{ $message }}</p>\n",
            ),
            ("resources/views/page.blade.php", template),
        ],
    );
    let root = backend.workspace_root().read().clone().unwrap();
    let uri = Url::from_file_path(root.join("resources/views/page.blade.php")).unwrap();
    (backend, dir, uri)
}

/// Complete at `line`/`character` without claiming any trigger character:
/// a component tag is typed one ordinary letter at a time.
async fn complete_typed(
    backend: &phpantom_lsp::Backend,
    uri: &Url,
    line: u32,
    character: u32,
) -> Vec<CompletionItem> {
    let params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position { line, character },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: Some(CompletionContext {
            trigger_kind: CompletionTriggerKind::INVOKED,
            trigger_character: None,
        }),
    };
    match backend.completion(params).await.unwrap() {
        Some(CompletionResponse::Array(items)) => items,
        Some(CompletionResponse::List(list)) => list.items,
        None => Vec::new(),
    }
}

fn labels(items: &[CompletionItem]) -> Vec<&str> {
    items.iter().map(|i| i.label.as_str()).collect()
}

#[tokio::test]
async fn an_x_opening_offers_every_component_the_project_ships() {
    let template = "<x-";
    let (backend, _dir, uri) = component_workspace(template);
    open_document(&backend, &uri, "blade", template).await;

    let items = complete_typed(&backend, &uri, 0, 3).await;
    let labels = labels(&items);
    assert!(
        labels.contains(&"alert") && labels.contains(&"forms.input"),
        "expected the class-based components, got: {labels:?}"
    );
    assert!(
        labels.contains(&"banner"),
        "expected the anonymous component template, got: {labels:?}"
    );
    assert!(
        !labels.contains(&"counter"),
        "a Livewire class is not an <x-…> component, got: {labels:?}"
    );
}

#[tokio::test]
async fn a_class_backed_component_is_offered_as_the_class_it_names() {
    let template = "<x-";
    let (backend, _dir, uri) = component_workspace(template);
    open_document(&backend, &uri, "blade", template).await;

    let items = complete_typed(&backend, &uri, 0, 3).await;
    let alert = items.iter().find(|i| i.label == "alert").expect("no alert");
    assert_eq!(alert.kind, Some(CompletionItemKind::CLASS));
    assert_eq!(
        alert.detail.as_deref(),
        Some("\\App\\View\\Components\\Alert")
    );

    let banner = items
        .iter()
        .find(|i| i.label == "banner")
        .expect("no banner");
    assert_eq!(banner.kind, Some(CompletionItemKind::MODULE));
}

#[tokio::test]
async fn a_partly_typed_component_name_filters_the_list() {
    let template = "<x-for";
    let (backend, _dir, uri) = component_workspace(template);
    open_document(&backend, &uri, "blade", template).await;

    let items = complete_typed(&backend, &uri, 0, 6).await;
    assert_eq!(labels(&items), vec!["forms.input"]);
    // The edit replaces what is typed rather than appending to it, since
    // an editor would not treat `for` as part of the same word as the
    // dotted name that replaces it.
    let edit = match items[0].text_edit.as_ref().expect("no text edit") {
        CompletionTextEdit::Edit(edit) => edit,
        other => panic!("expected a plain edit, got {other:?}"),
    };
    assert_eq!(edit.new_text, "forms.input");
    assert_eq!(edit.range.start.character, 3);
    assert_eq!(edit.range.end.character, 6);
}

#[tokio::test]
async fn a_livewire_opening_offers_the_livewire_index() {
    let template = "<livewire:";
    let (backend, _dir, uri) = component_workspace(template);
    open_document(&backend, &uri, "blade", template).await;

    let items = complete_typed(&backend, &uri, 0, 10).await;
    let labels = labels(&items);
    assert_eq!(
        labels,
        vec!["counter"],
        "only Livewire classes answer a <livewire:…> tag"
    );
}

#[tokio::test]
async fn a_constructor_parameter_is_offered_as_an_attribute() {
    let template = "<x-alert ";
    let (backend, _dir, uri) = component_workspace(template);
    open_document(&backend, &uri, "blade", template).await;

    let items = complete_typed(&backend, &uri, 0, 9).await;
    let labels = labels(&items);
    assert!(
        labels.contains(&"type") && labels.contains(&":type"),
        "expected both the literal and the bound form, got: {labels:?}"
    );
    // A camelCase parameter is written as the kebab-case attribute that
    // fills it.
    assert!(
        labels.contains(&"dismiss-label"),
        "expected the kebab-case spelling, got: {labels:?}"
    );
    let bound = items.iter().find(|i| i.label == ":type").expect("no :type");
    let edit = match bound.text_edit.as_ref().expect("no text edit") {
        CompletionTextEdit::Edit(edit) => edit,
        other => panic!("expected a plain edit, got {other:?}"),
    };
    assert_eq!(edit.new_text, ":type=\"$1\"");
}

#[tokio::test]
async fn a_required_attribute_is_offered_before_an_optional_one() {
    let template = "<x-alert ";
    let (backend, _dir, uri) = component_workspace(template);
    open_document(&backend, &uri, "blade", template).await;

    let items = complete_typed(&backend, &uri, 0, 9).await;
    let sort_of = |label: &str| {
        items
            .iter()
            .find(|i| i.label == label)
            .and_then(|i| i.sort_text.clone())
            .unwrap_or_else(|| panic!("no {label}"))
    };
    assert!(
        sort_of("type") < sort_of("dismiss-label"),
        "a tag missing a required attribute is short an argument, so it comes first"
    );
}

#[tokio::test]
async fn a_colon_narrows_the_attributes_to_the_bound_form() {
    let template = "<x-alert :";
    let (backend, _dir, uri) = component_workspace(template);
    open_document(&backend, &uri, "blade", template).await;

    let items = complete_typed(&backend, &uri, 0, 10).await;
    let labels = labels(&items);
    assert!(
        labels.iter().all(|label| label.starts_with(':')),
        "got: {labels:?}"
    );
    assert!(labels.contains(&":type"), "got: {labels:?}");
}

#[tokio::test]
async fn an_anonymous_components_props_are_its_attributes() {
    let template = "<x-banner ";
    let (backend, _dir, uri) = component_workspace(template);
    open_document(&backend, &uri, "blade", template).await;

    let items = complete_typed(&backend, &uri, 0, 10).await;
    let labels = labels(&items);
    assert!(
        labels.contains(&"headline") && labels.contains(&"sub-headline"),
        "expected the template's @props entries, got: {labels:?}"
    );
}

#[tokio::test]
async fn an_anonymous_component_without_props_offers_the_names_it_reads() {
    let template = "<x-hero ";
    let (backend, _dir, uri) = component_workspace(template);
    open_document(&backend, &uri, "blade", template).await;

    let items = complete_typed(&backend, &uri, 0, 8).await;
    let labels = labels(&items);
    assert!(
        labels.contains(&"title") && labels.contains(&"rows"),
        "expected the template's undeclared reads, got: {labels:?}"
    );
    // A read of a camelCase variable is written as the kebab-case
    // attribute Blade camel-cases back into it.
    assert!(
        labels.contains(&"sub-title"),
        "expected the kebab-case spelling of $subTitle, got: {labels:?}"
    );
}

#[tokio::test]
async fn a_name_the_component_template_supplies_itself_is_not_an_attribute() {
    let template = "<x-hero ";
    let (backend, _dir, uri) = component_workspace(template);
    open_document(&backend, &uri, "blade", template).await;

    let items = complete_typed(&backend, &uri, 0, 8).await;
    let labels = labels(&items);
    for name in ["caption", "theme", "row", "loop", "slot"] {
        assert!(
            !labels.contains(&name),
            "{name} is not the tag's to pass, got: {labels:?}"
        );
    }
}

#[tokio::test]
async fn a_declared_prop_keeps_its_default_beside_the_names_it_leaves_out() {
    let template = "<x-notice ";
    let (backend, _dir, uri) = component_workspace(template);
    open_document(&backend, &uri, "blade", template).await;

    let items = complete_typed(&backend, &uri, 0, 10).await;
    let level = items
        .iter()
        .find(|item| item.label == "level")
        .unwrap_or_else(|| panic!("expected the declared prop, got: {:?}", labels(&items)));
    assert_eq!(level.detail.as_deref(), Some("= 'info'"));
    assert!(
        labels(&items).contains(&"message"),
        "expected the name @props leaves out, got: {:?}",
        labels(&items)
    );
}

#[tokio::test]
async fn a_livewire_tag_offers_its_mount_parameters_and_public_properties() {
    let template = "<livewire:counter ";
    let (backend, _dir, uri) = component_workspace(template);
    open_document(&backend, &uri, "blade", template).await;

    let items = complete_typed(&backend, &uri, 0, 18).await;
    let labels = labels(&items);
    assert!(
        labels.contains(&"start") && labels.contains(&"total"),
        "expected mount()'s parameters and the public properties, got: {labels:?}"
    );
}

#[tokio::test]
async fn attribute_completion_does_not_fire_inside_an_attribute_value() {
    let template = "<x-alert type=\"da\" />";
    let (backend, _dir, uri) = component_workspace(template);
    open_document(&backend, &uri, "blade", template).await;

    // Cursor between `da` and the closing quote.
    let items = complete_typed(&backend, &uri, 0, 17).await;
    assert!(
        !labels(&items).contains(&"type"),
        "the value is PHP or text, not an attribute name, got: {:?}",
        labels(&items)
    );
}

// ── Virtual-PHP completion edits translated back to Blade coordinates ───

#[tokio::test]
async fn an_include_view_name_edit_lands_on_the_directive_not_the_prologue() {
    let composer = r#"{"autoload": {"psr-4": {"App\\": "app/"}}}"#;
    let (backend, _dir) = create_psr4_workspace(
        composer,
        &[
            (
                "resources/views/partials/header.blade.php",
                "<div>header</div>",
            ),
            ("resources/views/page.blade.php", "@include('"),
        ],
    );
    let root = backend.workspace_root().read().clone().unwrap();
    let uri = Url::from_file_path(root.join("resources/views/page.blade.php")).unwrap();
    let template = "@include('";
    open_document(&backend, &uri, "blade", template).await;

    // `@include('` is lowered into the virtual PHP's prologue-shifted body,
    // so an untranslated edit would land several lines below line 0.
    let items = complete_typed(&backend, &uri, 0, 10).await;
    let item = items
        .iter()
        .find(|i| i.label == "partials.header")
        .unwrap_or_else(|| panic!("expected the partial view name, got: {:?}", labels(&items)));
    let edit = match item.text_edit.as_ref().expect("no text edit") {
        CompletionTextEdit::Edit(edit) => edit,
        other => panic!("expected a plain edit, got {other:?}"),
    };
    assert_eq!(
        edit.range.start.line, 0,
        "the edit must land on the template's own line, not the virtual PHP prologue"
    );
}

#[tokio::test]
async fn a_closed_tag_leaves_completion_to_the_rest_of_the_pipeline() {
    let template = "<x-alert type=\"danger\" />\n@\n";
    let (backend, _dir, uri) = component_workspace(template);
    open_document(&backend, &uri, "blade", template).await;

    // The `@` on the second line is outside the tag, so directive
    // completion must still own it.
    let items = complete_typed(&backend, &uri, 1, 1).await;
    assert!(
        labels(&items).contains(&"@if"),
        "expected directive completion, got: {:?}",
        labels(&items)
    );
}
