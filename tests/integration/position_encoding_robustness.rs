//! Requests at hostile positions (inside a surrogate pair, past the end of a
//! line or the file, on CRLF lines, after multibyte text) must never panic
//! and must still land on the right columns, plus the class and property
//! lookups hover and go-to-definition rely on. Several cases are adapted
//! from laravel-lsp's MIT-licensed test suite.

use crate::common::{
    LARAVEL_APP_COMPOSER, complete_at, complete_labels_at_opened_with_trigger,
    create_initialized_psr4_workspace, create_test_backend, definition_locations,
    goto_definition_at, hover_text_at, method_names, open_document, open_php, position_after,
    position_of, property_names,
};
use phpantom_lsp::Backend;
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

fn document_position(uri: &Url, line: u32, character: u32) -> TextDocumentPositionParams {
    TextDocumentPositionParams {
        text_document: TextDocumentIdentifier { uri: uri.clone() },
        position: Position { line, character },
    }
}

/// Send hover, completion, go-to-definition and find-references at one
/// position, discarding the answers. Returning at all is the assertion.
async fn every_request(backend: &Backend, uri: &Url, line: u32, character: u32) {
    let _ = backend
        .hover(HoverParams {
            text_document_position_params: document_position(uri, line, character),
            work_done_progress_params: WorkDoneProgressParams::default(),
        })
        .await;
    let _ = backend
        .completion(CompletionParams {
            text_document_position: document_position(uri, line, character),
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await;
    let _ = backend
        .goto_definition(GotoDefinitionParams {
            text_document_position_params: document_position(uri, line, character),
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await;
    let _ = backend
        .references(ReferenceParams {
            text_document_position: document_position(uri, line, character),
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: ReferenceContext {
                include_declaration: true,
            },
        })
        .await;
}

/// [`every_request`] at every UTF-16 column of every line of `text` (the
/// ones that split a surrogate pair included), a couple of columns past the
/// end of each line, and on lines past the end of the file.
async fn sweep(backend: &Backend, uri: &Url, text: &str) {
    let lines: Vec<&str> = text.split('\n').collect();
    for (line, content) in lines.iter().enumerate() {
        let width = content.encode_utf16().count() as u32;
        for character in 0..=width + 2 {
            every_request(backend, uri, line as u32, character).await;
        }
    }
    let past = lines.len() as u32;
    for line in [past, past + 5] {
        for character in [0, 1, 40] {
            every_request(backend, uri, line, character).await;
        }
    }
}

/// The UTF-16 column of every occurrence of `needle` on `line` of `text`.
fn utf16_columns_of(text: &str, line: u32, needle: &str) -> Vec<u32> {
    let content = text.split('\n').nth(line as usize).unwrap();
    content
        .match_indices(needle)
        .map(|(byte, _)| content[..byte].encode_utf16().count() as u32)
        .collect()
}

// ─── No panics at hostile positions ─────────────────────────────────────────

const MULTIBYTE_PHP: &str = "<?php
class Café {
    public string $naïve = 'é';
    public function façade(): static { return $this; }
    public static function wher(): static { return new static(); }
}
$café🎉 = new Café();
$café🎉->façade()->naïve;
echo '🎉🎉 café' . $café🎉->naïve;
Café::wher();
Modèl::wher
$café->ba
App\\Modèls\\Café::
🎉::find
$🎉->save
/* 🎉 */ $x = ['é' => '🎉'];
";

#[tokio::test]
async fn every_column_of_a_multibyte_php_file_answers_without_panicking() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///multibyte.php").unwrap();
    open_php(&backend, &uri, MULTIBYTE_PHP).await;

    sweep(&backend, &uri, MULTIBYTE_PHP).await;
}

#[tokio::test]
async fn every_column_of_a_crlf_php_file_answers_without_panicking() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///crlf_multibyte.php").unwrap();
    let text = MULTIBYTE_PHP.replace('\n', "\r\n");
    open_php(&backend, &uri, &text).await;

    sweep(&backend, &uri, &text).await;
}

const MULTIBYTE_BLADE: &str = "@php
/** @var \\Logger $logger */
@endphp
<x-café-🎉 class=\"é\" />
@if ($logger) 🎉 café @endif
<p>🎉 {{ $logger->info() }} é {!! $logger->warn() !!}</p>
{{-- 🎉 @if café --}}
@foreach ([$logger] as $é)
    {{ $é->info() }} {{ $loop->index }} 🎉
@endforeach
<p>foo@if 🎉</p>
";

#[tokio::test]
async fn every_column_of_a_multibyte_blade_template_answers_without_panicking() {
    let backend = create_test_backend();
    open_php(
        &backend,
        &Url::parse("file:///Logger.php").unwrap(),
        "<?php\nclass Logger { public function info(): void {} public function warn(): void {} }\n",
    )
    .await;
    let uri = Url::parse("file:///multibyte.blade.php").unwrap();
    open_document(&backend, &uri, "blade", MULTIBYTE_BLADE).await;

    sweep(&backend, &uri, MULTIBYTE_BLADE).await;
}

#[tokio::test]
async fn every_column_of_a_crlf_blade_template_answers_without_panicking() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///crlf.blade.php").unwrap();
    let text = MULTIBYTE_BLADE.replace('\n', "\r\n");
    open_document(&backend, &uri, "blade", &text).await;

    sweep(&backend, &uri, &text).await;
}

/// The Laravel string-key contexts (config, route, view, translation and
/// env keys) each slice the line around the cursor.
#[tokio::test]
async fn every_column_of_multibyte_laravel_string_keys_answers_without_panicking() {
    let probe = "<?php
namespace App;
class Probe {
    public function run(): void {
        config('app.café🎉');
        route('users.café🎉');
        view('café.🎉.index');
        __('messages.café🎉');
        trans('messages.é');
        env('CAFÉ🎉');
        asset('css/🎉.css');
        $café🎉 = new Probe();
        $café🎉->run();
    }
}
";
    let (backend, _dir, uri) = create_initialized_psr4_workspace(
        LARAVEL_APP_COMPOSER,
        &[
            (
                "config/app.php",
                "<?php\nreturn ['name' => 'Acme', 'café🎉' => 'x'];\n",
            ),
            (
                "routes/web.php",
                "<?php\nuse Illuminate\\Support\\Facades\\Route;\nRoute::get('/', fn () => 1)->name('users.café🎉');\n",
            ),
            ("resources/views/café/index.blade.php", "<p>🎉</p>\n"),
            ("lang/en/messages.php", "<?php\nreturn ['café🎉' => 'x'];\n"),
            (".env", "CAFÉ🎉=1\n"),
            ("app/Probe.php", probe),
        ],
        "app/Probe.php",
    )
    .await;

    sweep(&backend, &uri, probe).await;
}

#[tokio::test]
async fn requests_on_an_empty_document_answer_without_panicking() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///empty.php").unwrap();
    open_php(&backend, &uri, "").await;

    sweep(&backend, &uri, "").await;
}

#[tokio::test]
async fn requests_on_a_document_of_only_multibyte_text_answer_without_panicking() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///emoji.php").unwrap();
    let text = "<?php // 🎉🎉🎉\n🎉";
    open_php(&backend, &uri, text).await;

    sweep(&backend, &uri, text).await;
}

#[tokio::test]
async fn the_largest_possible_column_answers_without_panicking() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///huge_column.php").unwrap();
    let text = "<?php\n$s = '🎉';\n$s;\n";
    open_php(&backend, &uri, text).await;

    for line in [0, 1, 2, 3, u32::MAX] {
        every_request(&backend, &uri, line, u32::MAX).await;
    }
    every_request(&backend, &uri, u32::MAX, 0).await;
}

// ─── Right answers past multibyte text ──────────────────────────────────────

const GREETER: &str = "<?php
class Greeter {
    public function wave(): string { return ''; }
}
$g = new Greeter(); $s = '🎉 café'; $g->wave(); /* é */ $g->wave();
";

#[tokio::test]
async fn hover_after_an_emoji_on_the_same_line_reads_the_member() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///greeter_hover.php").unwrap();
    open_php(&backend, &uri, GREETER).await;

    let at = position_of(GREETER, "wave();");
    let hover = hover_text_at(&backend, &uri, at.line, at.character + 1)
        .await
        .unwrap_or_default();
    assert!(hover.contains("wave"), "got: {hover}");
}

#[tokio::test]
async fn definition_after_an_emoji_on_the_same_line_lands_on_the_member() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///greeter_definition.php").unwrap();
    open_php(&backend, &uri, GREETER).await;

    let at = position_of(GREETER, "wave();");
    let locations =
        definition_locations(goto_definition_at(&backend, &uri, at.line, at.character + 1).await);
    assert_eq!(
        locations.first().map(|l| l.range.start.line),
        Some(2),
        "got: {locations:?}"
    );
}

#[tokio::test]
async fn completion_after_multibyte_text_on_the_same_line_offers_members() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///greeter_completion.php").unwrap();
    let text = "<?php
class Greeter {
    public function wave(): string { return ''; }
}
$g = new Greeter(); $s = '🎉 café'; $g->";
    let at = position_after(text, "$g->");

    let items = complete_at(&backend, &uri, text, at.line, at.character).await;
    assert!(
        method_names(&items).contains(&"wave"),
        "got: {:?}",
        method_names(&items)
    );
}

#[tokio::test]
async fn reference_ranges_after_multibyte_text_are_utf16_columns() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///greeter_references.php").unwrap();
    open_php(&backend, &uri, GREETER).await;

    let at = position_of(GREETER, "wave();");
    let locations = backend
        .references(ReferenceParams {
            text_document_position: document_position(&uri, at.line, at.character + 1),
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: ReferenceContext {
                include_declaration: false,
            },
        })
        .await
        .unwrap()
        .unwrap_or_default();

    let mut found: Vec<(u32, u32)> = locations
        .iter()
        .filter(|l| l.range.start.line == 4)
        .map(|l| (l.range.start.character, l.range.end.character))
        .collect();
    found.sort_unstable();
    let expected: Vec<(u32, u32)> = utf16_columns_of(GREETER, 4, "wave();")
        .into_iter()
        .map(|start| (start, start + 4))
        .collect();
    assert_eq!(found, expected, "got: {locations:?}");
}

#[tokio::test]
#[ignore = "known gap: hover finds no symbol at a non-ASCII identifier"]
async fn a_multibyte_variable_name_resolves_its_type() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///multibyte_variable.php").unwrap();
    let text = "<?php
class Greeter {
    public function wave(): string { return ''; }
}
$café = new Greeter();
$café->";
    let at = position_after(text, "$café->");

    let items = complete_at(&backend, &uri, text, at.line, at.character).await;
    assert!(
        method_names(&items).contains(&"wave"),
        "got: {:?}",
        method_names(&items)
    );

    let hover = hover_text_at(&backend, &uri, at.line, 2)
        .await
        .unwrap_or_default();
    assert!(hover.contains("Greeter"), "got: {hover}");
}

#[tokio::test]
#[ignore = "known gap: hover finds no symbol at a non-ASCII identifier"]
async fn a_multibyte_class_name_resolves_its_static_members() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///multibyte_class.php").unwrap();
    let text = "<?php
class Modèl {
    public static function where(): static { return new static(); }
}
Modèl::wh";
    let at = position_after(text, "Modèl::wh");

    let items = complete_at(&backend, &uri, text, at.line, at.character).await;
    assert!(
        method_names(&items).contains(&"where"),
        "got: {:?}",
        method_names(&items)
    );

    let hover = hover_text_at(&backend, &uri, at.line, 2)
        .await
        .unwrap_or_default();
    assert!(hover.contains("Modèl"), "got: {hover}");
}

// ─── CRLF line endings ──────────────────────────────────────────────────────

const GREETER_CRLF: &str = "<?php\r
class Greeter {\r
    public function wave(): string { return ''; }\r
}\r
$g = new Greeter();\r
$g->wave(); $g->wave();\r
";

#[tokio::test]
async fn hover_on_a_crlf_line_reads_the_member() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///crlf_hover.php").unwrap();
    open_php(&backend, &uri, GREETER_CRLF).await;

    let hover = hover_text_at(&backend, &uri, 5, 5)
        .await
        .unwrap_or_default();
    assert!(hover.contains("wave"), "got: {hover}");
}

#[tokio::test]
async fn definition_on_a_crlf_line_lands_on_the_declaring_line() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///crlf_definition.php").unwrap();
    open_php(&backend, &uri, GREETER_CRLF).await;

    let locations = definition_locations(goto_definition_at(&backend, &uri, 5, 5).await);
    assert_eq!(
        locations.first().map(|l| l.range.start.line),
        Some(2),
        "got: {locations:?}"
    );
}

#[tokio::test]
async fn completion_on_a_crlf_line_offers_members() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///crlf_completion.php").unwrap();
    let text = "<?php\r\nclass Greeter {\r\n    public function wave(): string { return ''; }\r\n}\r\n$g = new Greeter();\r\n$g->\r\n";

    let items = complete_at(&backend, &uri, text, 5, 4).await;
    assert!(
        method_names(&items).contains(&"wave"),
        "got: {:?}",
        method_names(&items)
    );
}

#[tokio::test]
async fn reference_ranges_on_crlf_lines_are_not_shifted_by_the_carriage_return() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///crlf_references.php").unwrap();
    open_php(&backend, &uri, GREETER_CRLF).await;

    let locations = backend
        .references(ReferenceParams {
            text_document_position: document_position(&uri, 5, 5),
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: ReferenceContext {
                include_declaration: false,
            },
        })
        .await
        .unwrap()
        .unwrap_or_default();

    let mut found: Vec<(u32, u32, u32)> = locations
        .iter()
        .map(|l| {
            (
                l.range.start.line,
                l.range.start.character,
                l.range.end.character,
            )
        })
        .collect();
    found.sort_unstable();
    assert_eq!(found, vec![(5, 4, 8), (5, 16, 20)], "got: {locations:?}");
}

#[tokio::test]
async fn a_member_in_a_crlf_blade_template_leads_to_its_declaration() {
    let backend = create_test_backend();
    open_php(
        &backend,
        &Url::parse("file:///Logger.php").unwrap(),
        "<?php\nclass Logger {\n    public function info(): void {}\n}\n",
    )
    .await;
    let uri = Url::parse("file:///crlf_view.blade.php").unwrap();
    let text = "@php\r\n/** @var \\Logger $logger */\r\n@endphp\r\n<p>🎉</p>\r\n<p>{{ $logger->info() }}</p>\r\n";
    open_document(&backend, &uri, "blade", text).await;

    let at = position_of(text, "info()");
    let locations =
        definition_locations(goto_definition_at(&backend, &uri, at.line, at.character + 1).await);
    assert_eq!(
        locations
            .first()
            .map(|l| (l.uri.as_str(), l.range.start.line)),
        Some(("file:///Logger.php", 2)),
        "got: {locations:?}"
    );
}

// ─── Class and property lookups ─────────────────────────────────────────────

#[tokio::test]
async fn hover_on_a_property_joins_a_multi_line_summary() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///summary.php").unwrap();
    let text = "<?php
class Account {
    /**
     * The account's email address.
     * Used for password recovery and notifications.
     */
    public string $email = '';
    public function show(): void { echo $this->email; }
}
";
    open_php(&backend, &uri, text).await;

    let at = position_of(text, "email; }");
    let hover = hover_text_at(&backend, &uri, at.line, at.character + 1)
        .await
        .unwrap_or_default();
    assert!(
        hover.contains("The account's email address.")
            && hover.contains("Used for password recovery"),
        "got: {hover}"
    );
}

#[tokio::test]
async fn hover_on_a_property_does_not_borrow_an_earlier_propertys_docblock() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///borrowed_doc.php").unwrap();
    let text = "<?php
class Account {
    /**
     * This describes something else.
     */
    public string $other = '';

    public string $email = '';
    public function show(): void { echo $this->email; }
}
";
    open_php(&backend, &uri, text).await;

    let at = position_of(text, "email; }");
    let hover = hover_text_at(&backend, &uri, at.line, at.character + 1)
        .await
        .unwrap_or_default();
    assert!(hover.contains("email"), "got: {hover}");
    assert!(
        !hover.contains("describes something else"),
        "the docblock belongs to $other, got: {hover}"
    );
}

#[tokio::test]
async fn definition_does_not_match_a_property_by_name_prefix() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///prefix.php").unwrap();
    let text = "<?php
class Account {
    public $name_with_extra;
}
function probe(Account $a): void { echo $a->name; }
";
    open_php(&backend, &uri, text).await;

    let at = position_of(text, "name; }");
    let locations =
        definition_locations(goto_definition_at(&backend, &uri, at.line, at.character + 1).await);
    assert!(
        locations.is_empty(),
        "$name_with_extra is not $name, got: {locations:?}"
    );
}

#[tokio::test]
async fn hover_on_a_class_ignores_the_class_keyword_inside_a_string() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///string_class.php").unwrap();
    let text = "<?php
class Bar {}
$x = 'class FakeClass';
final class Real extends Bar {}
new Real();
";
    open_php(&backend, &uri, text).await;

    let at = position_of(text, "Real();");
    let hover = hover_text_at(&backend, &uri, at.line, at.character + 1)
        .await
        .unwrap_or_default();
    assert!(
        hover.contains("final class Real extends Bar"),
        "got: {hover}"
    );
}

/// PHP rejects the second declaration, but the file is still being edited:
/// answer from the first rather than listing the name twice.
#[tokio::test]
async fn a_property_declared_twice_is_offered_once() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///twin.php").unwrap();
    let text = "<?php
class Twin {
    public string $foo = '';
    public int $foo = 0;
}
$t = new Twin();
$t->";
    let at = position_after(text, "$t->");

    let items = complete_at(&backend, &uri, text, at.line, at.character).await;
    let properties = property_names(&items);
    assert_eq!(
        properties.iter().filter(|p| **p == "foo").count(),
        1,
        "got: {properties:?}"
    );
}

// ─── The variable under the cursor in a Blade echo ──────────────────────────

const FORM_VIEW: &str = "@php
/** @var \\Form $form */
@endphp
<p>{{ $form->name }}</p>
<p>Hello world</p>
<p>{{ $ }}</p>
";

async fn form_view() -> (Backend, Url) {
    let backend = create_test_backend();
    open_php(
        &backend,
        &Url::parse("file:///Form.php").unwrap(),
        "<?php\nclass Form {\n    public string $name = '';\n}\n",
    )
    .await;
    let uri = Url::parse("file:///form.blade.php").unwrap();
    open_document(&backend, &uri, "blade", FORM_VIEW).await;
    (backend, uri)
}

#[tokio::test]
async fn hover_anywhere_on_an_echoed_variable_reads_it() {
    let (backend, uri) = form_view().await;

    // `<p>{{ $form->name }}</p>`: the `$`, the first letter, and mid-name.
    for character in [6, 7, 9] {
        let hover = hover_text_at(&backend, &uri, 3, character)
            .await
            .unwrap_or_default();
        assert!(hover.contains("Form"), "column {character}, got: {hover}");
    }
}

#[tokio::test]
async fn hover_on_an_echoed_property_reads_the_property() {
    let (backend, uri) = form_view().await;

    let hover = hover_text_at(&backend, &uri, 3, 15)
        .await
        .unwrap_or_default();
    assert!(hover.contains("name"), "got: {hover}");
    let locations = definition_locations(goto_definition_at(&backend, &uri, 3, 15).await);
    assert_eq!(
        locations
            .first()
            .map(|l| (l.uri.as_str(), l.range.start.line)),
        Some(("file:///Form.php", 2)),
        "got: {locations:?}"
    );
}

#[tokio::test]
async fn hover_on_plain_template_text_reads_nothing() {
    let (backend, uri) = form_view().await;

    assert_eq!(hover_text_at(&backend, &uri, 4, 5).await, None);
}

#[tokio::test]
async fn hover_past_the_end_of_an_echo_line_reads_nothing() {
    let (backend, uri) = form_view().await;

    let hover = hover_text_at(&backend, &uri, 3, 100)
        .await
        .unwrap_or_default();
    assert!(!hover.contains("Form"), "got: {hover}");
}

#[tokio::test]
async fn a_bare_dollar_in_an_echo_offers_the_templates_variables() {
    let (backend, uri) = form_view().await;

    let labels = complete_labels_at_opened_with_trigger(&backend, &uri, 5, 7, "$").await;
    assert!(labels.iter().any(|l| l == "$form"), "got: {labels:?}");
}
