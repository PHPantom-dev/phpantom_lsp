//! Code actions in a Blade template.
//!
//! Every action is planned against the virtual PHP the template lowers to
//! and moved back into the template, so each test goes through the server
//! request and applies the edits the editor would get to the template's
//! own text.

use crate::common::{
    APP_PSR4_COMPOSER, BLADE_COMPONENT_COMPOSER, ILLUMINATE_COMPONENT_STUB,
    LIVEWIRE_COMPONENT_STUB, code_actions_via_server, create_psr4_workspace, open_document,
    workspace_uri,
};
use phpantom_lsp::Backend;
use tower_lsp_server::gen_lsp_types::*;

const MONEY: &str = "<?php\nnamespace App\\Support;\n\
    class Money { public static function zero(): self { return new self(); } }\n";

const CLOCK: &str = "<?php\nnamespace App\\Support;\n\
    class Clock { public static function now(): self { return new self(); } }\n";

const ALERT: &str = "<?php\nnamespace App\\View\\Components;\n\
    use Illuminate\\View\\Component;\n\
    class Alert extends Component {\n\
        public function __construct(public string $type) {}\n\
        public function render() {}\n\
    }\n";

const VIEW: &str = "resources/views/page.blade.php";

/// A workspace holding the support classes, the component classes, and
/// `template` as the view at [`VIEW`], opened the way an editor opens it.
async fn open_template(template: &str) -> (Backend, tempfile::TempDir, Uri) {
    let (backend, dir) = create_psr4_workspace(
        BLADE_COMPONENT_COMPOSER,
        &[
            ("app/Support/Money.php", MONEY),
            ("app/Support/Clock.php", CLOCK),
            (
                "stubs/Illuminate/View/Component.php",
                ILLUMINATE_COMPONENT_STUB,
            ),
            ("app/View/Components/Alert.php", ALERT),
            (VIEW, template),
        ],
    );
    declare_support_classes(&backend);
    let uri = workspace_uri(&backend, VIEW);
    open_document(&backend, &uri, "blade", template).await;
    (backend, dir, uri)
}

/// Parse the support classes so they are import candidates.
fn declare_support_classes(backend: &Backend) {
    for (relative, content) in [
        ("app/Support/Money.php", MONEY),
        ("app/Support/Clock.php", CLOCK),
    ] {
        backend.update_ast(workspace_uri(backend, relative).as_str(), content);
    }
}

fn range(line: u32, start: u32, end: u32) -> Range {
    Range::new(Position::new(line, start), Position::new(line, end))
}

/// The range of the first `needle` on `line` of `template`.
fn range_of(template: &str, line: u32, needle: &str) -> Range {
    let text = template
        .lines()
        .nth(line as usize)
        .expect("the line exists");
    let start = text.find(needle).expect("the needle is on the line") as u32;
    range(line, start, start + needle.len() as u32)
}

async fn actions(backend: &Backend, uri: &Uri, range: Range) -> Vec<CodeAction> {
    code_actions_via_server(backend, uri, range)
        .await
        .unwrap_or_default()
        .into_iter()
        .filter_map(|action| match action {
            CodeActionResponse::CodeAction(action) => Some(action),
            CodeActionResponse::Command(_) => None,
        })
        .collect()
}

fn titles(actions: &[CodeAction]) -> Vec<&str> {
    actions.iter().map(|action| action.title.as_str()).collect()
}

fn find<'a>(actions: &'a [CodeAction], prefix: &str) -> &'a CodeAction {
    actions
        .iter()
        .find(|action| action.title.starts_with(prefix))
        .unwrap_or_else(|| panic!("expected {prefix:?}, got: {:?}", titles(actions)))
}

/// Apply `action`'s edits to the template, which must be the only file it
/// edits, the way the editor does.
fn apply(template: &str, uri: &Uri, action: &CodeAction) -> String {
    assert!(
        action.data.is_none(),
        "a template's action is resolved before it is offered: {}",
        action.title
    );
    let edit = action
        .edit
        .as_ref()
        .unwrap_or_else(|| panic!("{} has an edit", action.title));
    let changes = edit.changes.as_ref().expect("the edit has changes");
    assert_eq!(
        changes.keys().collect::<Vec<_>>(),
        [uri],
        "the action edits the template alone"
    );
    apply_in_order(template, &changes[uri])
}

/// Apply `edits` the way the LSP specifies: every range refers to the
/// original text, and insertions at the same position go in in the order
/// they are listed.
fn apply_in_order(text: &str, edits: &[TextEdit]) -> String {
    let offset = |position: Position| {
        text.split_inclusive('\n')
            .take(position.line as usize)
            .map(str::len)
            .sum::<usize>()
            + position.character as usize
    };
    let mut ordered: Vec<(usize, &TextEdit)> = edits.iter().enumerate().collect();
    ordered.sort_by(|(a_index, a), (b_index, b)| {
        b.range.start.cmp(&a.range.start).then(b_index.cmp(a_index))
    });
    let mut result = text.to_string();
    for (_, edit) in ordered {
        result.replace_range(
            offset(edit.range.start)..offset(edit.range.end),
            &edit.new_text,
        );
    }
    result
}

// ── Imports ─────────────────────────────────────────────────────────────────

/// Blade compiles `@use` to a PHP `use` where it stands, and PHP rejects one
/// inside a block, so a template whose first line opens one takes the
/// import above it.
#[tokio::test]
async fn import_class_goes_above_the_block_the_first_line_opens() {
    let template = "@if ($show)\n    {{ Money::zero() }}\n@endif\n";
    let (backend, _dir, uri) = open_template(template).await;

    let actions = actions(&backend, &uri, range_of(template, 1, "Money")).await;
    let import = find(&actions, "Import `App\\Support\\Money`");

    assert_eq!(
        apply(template, &uri, import),
        "@use('App\\Support\\Money')\n@if ($show)\n    {{ Money::zero() }}\n@endif\n"
    );
}

/// A component tag lowers to a constructor call naming the component class
/// in full, and the prologue declares `$errors` with a qualified class, but
/// neither is a name the template wrote, so importing every qualified name
/// imports only the template's own.
#[tokio::test]
async fn import_all_qualified_symbols_skips_the_names_the_lowering_wrote() {
    let template = "<x-alert type=\"danger\">\n\
                    {{ \\App\\Support\\Money::zero() }} {{ \\App\\Support\\Clock::now() }}\n\
                    </x-alert>\n";
    let (backend, _dir, uri) = open_template(template).await;

    let actions = actions(&backend, &uri, range_of(template, 1, "Money")).await;
    let all = find(&actions, "Import all qualified symbols");

    assert_eq!(
        apply(template, &uri, all),
        "@use('App\\Support\\Clock')\n@use('App\\Support\\Money')\n\
         <x-alert type=\"danger\">\n{{ Money::zero() }} {{ Clock::now() }}\n</x-alert>\n"
    );
}

/// The argument of a `@php(...)` one-liner is the template's own PHP, so a
/// qualified name in it is shortened where it is written.
#[tokio::test]
async fn import_a_qualified_name_in_a_php_one_liner() {
    let template = "{{-- a note --}}\n@php($money = \\App\\Support\\Money::zero())\n";
    let (backend, _dir, uri) = open_template(template).await;

    let actions = actions(&backend, &uri, range_of(template, 1, "Money")).await;
    let import = find(&actions, "Import class `App\\Support\\Money`");

    assert_eq!(
        apply(template, &uri, import),
        "@use('App\\Support\\Money')\n{{-- a note --}}\n@php($money = Money::zero())\n"
    );
}

/// A Livewire view's body is a method of a class synthesized to extend the
/// component, and an import still goes into the template's own text.
#[tokio::test]
async fn import_class_in_a_livewire_view() {
    let template = "<div>{{ Money::zero() }}</div>\n";
    let relative = "resources/views/livewire/order-list.blade.php";
    let (backend, _dir) = create_psr4_workspace(
        BLADE_COMPONENT_COMPOSER,
        &[
            ("stubs/Livewire/Component.php", LIVEWIRE_COMPONENT_STUB),
            ("app/Support/Money.php", MONEY),
            (
                "app/Livewire/OrderList.php",
                "<?php\nnamespace App\\Livewire;\n\
                 use Livewire\\Component;\n\
                 class OrderList extends Component {\n\
                     public int $page = 1;\n\
                     public function render() {}\n\
                 }\n",
            ),
            (relative, template),
        ],
    );
    declare_support_classes(&backend);
    let uri = workspace_uri(&backend, relative);
    open_document(&backend, &uri, "blade", template).await;
    assert!(
        backend
            .blade_virtual_php(uri.as_str())
            .is_some_and(|php| php.contains("class __blade_scope_")),
        "the view must lower to a method of the synthesized subclass"
    );

    let actions = actions(&backend, &uri, range_of(template, 0, "Money")).await;
    let import = find(&actions, "Import `App\\Support\\Money`");

    assert_eq!(
        apply(template, &uri, import),
        "@use('App\\Support\\Money')\n<div>{{ Money::zero() }}</div>\n"
    );
}

/// An unused `@use` lowers to nothing in the template's body, so its
/// removal is planned against the directive the template wrote, and the
/// action carries the diagnostic in the template's coordinates.
#[tokio::test]
async fn remove_unused_use_directive() {
    let template = "@use('App\\Support\\Money')\n<p>hi</p>\n";
    let (backend, _dir, uri) = open_template(template).await;

    let actions = actions(&backend, &uri, range(0, 8, 8)).await;
    let remove = find(&actions, "Remove unused import");

    let diagnostics = remove.diagnostics.as_deref().unwrap_or_default();
    assert_eq!(
        diagnostics.iter().map(|d| d.range).collect::<Vec<_>>(),
        [range(0, 0, 25)],
        "the diagnostic is the one reported on the directive"
    );
    assert_eq!(apply(template, &uri, remove), "<p>hi</p>\n");
}

// ── Refactorings ────────────────────────────────────────────────────────────

/// A statement inside a `@php` block is the template's own PHP, so an
/// extraction lands inside the block.
#[tokio::test]
async fn extract_variable_inside_a_php_block() {
    let template = "@php\n    $total = strlen('abc') + 1;\n@endphp\n{{ $total }}\n";
    let (backend, _dir, uri) = open_template(template).await;

    let actions = actions(&backend, &uri, range_of(template, 1, "strlen('abc')")).await;
    let extract = find(&actions, "Extract variable");

    let extracted = apply(template, &uri, extract);
    assert!(
        extracted.starts_with("@php\n    $")
            && extracted.contains(" = strlen('abc');\n    $total = $")
            && extracted.ends_with(" + 1;\n@endphp\n{{ $total }}\n"),
        "the variable must be declared inside the block: {extracted:?}"
    );
}

/// A CRLF template's line breaks come out bare in the virtual PHP; the
/// edit still lands on the template's text, with its line terminator.
#[tokio::test]
async fn extract_variable_in_a_crlf_template() {
    let template = "<p>\r\n@php\r\n    $total = strlen('abc') + 1;\r\n@endphp\r\n</p>\r\n";
    let (backend, _dir, uri) = open_template(template).await;

    let actions = actions(&backend, &uri, range_of(template, 2, "strlen('abc')")).await;
    let extract = find(&actions, "Extract variable");

    let extracted = apply(template, &uri, extract);
    assert!(
        extracted.starts_with("<p>\r\n@php\r\n    $")
            && extracted.contains(" = strlen('abc');\r\n    $total = $")
            && extracted.ends_with(" + 1;\r\n@endphp\r\n</p>\r\n"),
        "the variable must be declared inside the block, with CRLF: {extracted:?}"
    );
}

/// The statement an echo's expression belongs to starts in the `echo` the
/// echo lowers to, which the template never wrote, so there is nowhere to
/// declare the variable and nothing is offered.
#[tokio::test]
async fn extract_variable_inside_an_echo_is_not_offered() {
    let template = "<p>{{ strlen('abc') }}</p>\n";
    let (backend, _dir, uri) = open_template(template).await;

    let actions = actions(&backend, &uri, range_of(template, 0, "strlen('abc')")).await;
    assert!(
        !titles(&actions)
            .iter()
            .any(|title| title.starts_with("Extract variable")),
        "got: {:?}",
        titles(&actions)
    );
}

/// The statement a `@php(...)` one-liner holds starts at its argument, and
/// a declaration inserted there would split the directive from it.
#[tokio::test]
async fn extract_variable_in_a_php_one_liner_is_not_offered() {
    let template = "@php($total = strlen('abc') + 1)\n{{ $total }}\n";
    let (backend, _dir, uri) = open_template(template).await;

    let actions = actions(&backend, &uri, range_of(template, 0, "strlen('abc')")).await;
    assert!(
        !titles(&actions)
            .iter()
            .any(|title| title.starts_with("Extract variable")),
        "got: {:?}",
        titles(&actions)
    );
}

// ── Stale lowering ──────────────────────────────────────────────────────────

/// The lowering is refreshed in the background, so it can lag the buffer;
/// an action planned against a lowering of older text would land on the
/// wrong template text, so nothing is offered until it catches up.
#[tokio::test]
async fn nothing_is_offered_for_a_template_edited_since_it_was_lowered() {
    let template = "@if ($show)\n    {{ Money::zero() }}\n@endif\n";
    let (backend, _dir, uri) = open_template(template).await;
    assert!(
        !actions(&backend, &uri, range_of(template, 1, "Money"))
            .await
            .is_empty(),
        "the import is offered while the lowering is current"
    );

    backend.open_files().write().insert(
        uri.to_string(),
        std::sync::Arc::new(format!("<p>typed</p>\n{template}")),
    );
    let actions = actions(&backend, &uri, range_of(template, 1, "Money")).await;
    assert!(actions.is_empty(), "got: {:?}", titles(&actions));
}

/// A plain PHP file is unaffected by the template handling: its import
/// still goes into a `use` statement.
#[tokio::test]
async fn a_php_file_still_imports_with_a_use_statement() {
    let (backend, _dir) =
        create_psr4_workspace(APP_PSR4_COMPOSER, &[("app/Support/Money.php", MONEY)]);
    declare_support_classes(&backend);
    let content = "<?php\nnamespace App;\n\nMoney::zero();\n";
    let uri = workspace_uri(&backend, "app/Page.php");
    open_document(&backend, &uri, "php", content).await;

    let actions = actions(&backend, &uri, range(3, 0, 5)).await;
    let import = find(&actions, "Import `App\\Support\\Money`");
    let edit = import.edit.as_ref().expect("an eager edit");
    let edits = &edit.changes.as_ref().expect("changes")[&uri];
    assert!(
        apply_in_order(content, edits).contains("use App\\Support\\Money;"),
        "got: {:?}",
        apply_in_order(content, edits)
    );
}
