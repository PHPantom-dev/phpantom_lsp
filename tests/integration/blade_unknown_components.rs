//! A component tag that names nothing: `<x-buttons.bakc />` and
//! `<livewire:orders.overveiw />` resolve to no class and no template, which
//! Laravel only reports once the template compiles.
//!
//! Only a name the project provably lacks is reported. A tag under a prefix
//! nothing here can enumerate, or one a provider may have registered under
//! a name the scan cannot read, is left alone.

use crate::common::{
    ILLUMINATE_COMPONENT_STUB, LIVEWIRE_COMPONENT_STUB, code_actions_via_server,
    create_psr4_workspace, initialize_with_resource_operations, message_text, open_document,
    workspace_uri,
};
use phpantom_lsp::Backend;
use tower_lsp_server::LanguageServer;
use tower_lsp_server::gen_lsp_types::*;

/// A Laravel application whose framework and Livewire base classes are the
/// stubs under `stubs/`.
const COMPOSER: &str = r#"{
    "require": { "laravel/framework": "^11.0" },
    "autoload": { "psr-4": {
        "App\\": "app/",
        "Illuminate\\": "stubs/Illuminate/",
        "Livewire\\": "stubs/Livewire/"
    } }
}"#;

const VIEW: &str = "resources/views/page.blade.php";

const PROVIDERS_PHP: &str = "<?php\nreturn [\n    App\\Providers\\AppServiceProvider::class,\n];\n";

fn class_component(namespace: &str, name: &str) -> String {
    format!(
        "<?php\nnamespace {namespace};\n\
         use Illuminate\\View\\Component;\n\
         class {name} extends Component {{\n\
             public function render() {{}}\n\
         }}\n"
    )
}

fn livewire_component(namespace: &str, name: &str) -> String {
    format!(
        "<?php\nnamespace {namespace};\n\
         use Livewire\\Component;\n\
         class {name} extends Component {{\n\
             public function render() {{}}\n\
         }}\n"
    )
}

/// A project with a class component, an anonymous one, a nested one, two
/// Livewire components, `extra` on top, and `template` at [`VIEW`].
async fn start(template: &str, extra: &[(&str, &str)]) -> (Backend, tempfile::TempDir, Uri) {
    let alert = class_component("App\\View\\Components", "Alert");
    let date_picker = class_component("App\\View\\Components\\Forms", "DatePicker");
    let counter = livewire_component("App\\Livewire", "Counter");
    let last_day = livewire_component("App\\Livewire\\Dashboard", "Last24HoursOrders");
    let mut files: Vec<(&str, &str)> = vec![
        (
            "stubs/Illuminate/View/Component.php",
            ILLUMINATE_COMPONENT_STUB,
        ),
        ("stubs/Livewire/Component.php", LIVEWIRE_COMPONENT_STUB),
        ("app/View/Components/Alert.php", &alert),
        ("app/View/Components/Forms/DatePicker.php", &date_picker),
        ("app/Livewire/Counter.php", &counter),
        ("app/Livewire/Dashboard/Last24HoursOrders.php", &last_day),
        (
            "resources/views/components/buttons/back.blade.php",
            "<a>{{ $slot }}</a>\n",
        ),
        (VIEW, template),
    ];
    files.extend_from_slice(extra);
    let (backend, dir) = create_psr4_workspace(COMPOSER, &files);
    initialize_with_resource_operations(&backend).await;
    backend.initialized(InitializedParams {}).await;
    let uri = workspace_uri(&backend, VIEW);
    open_document(&backend, &uri, "blade", template).await;
    (backend, dir, uri)
}

/// The unknown-component reports on the template, as `(code, message,
/// line, column)`.
fn unknown_components(backend: &Backend, uri: &Uri) -> Vec<(String, String, u32, u32)> {
    let virtual_php = backend
        .blade_virtual_php(uri.as_str())
        .expect("the template should be preprocessed");
    let mut diags = Vec::new();
    backend.collect_slow_diagnostics(uri.as_str(), &virtual_php, &mut diags);
    diags
        .into_iter()
        .filter_map(|d| match &d.code {
            Some(Code::String(code)) if code.ends_with("_component") => Some((
                code.clone(),
                message_text(&d.message).to_string(),
                d.range.start.line,
                d.range.start.character,
            )),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn a_misspelt_blade_component_is_reported_on_its_name() {
    let (backend, _dir, uri) = start(
        "<x-alert />\n\
         <x-buttons.bakc>Back</x-buttons.bakc>\n",
        &[],
    )
    .await;
    assert_eq!(
        unknown_components(&backend, &uri),
        vec![(
            "unknown_blade_component".to_string(),
            "Unknown Blade component: 'buttons.bakc'".to_string(),
            1,
            3
        )]
    );
}

#[tokio::test]
async fn a_misspelt_livewire_component_is_reported() {
    let (backend, _dir, uri) = start(
        "<livewire:counter />\n\
         <livewire:orders.overveiw />\n",
        &[],
    )
    .await;
    assert_eq!(
        unknown_components(&backend, &uri),
        vec![(
            "unknown_livewire_component".to_string(),
            "Unknown Livewire component: 'orders.overveiw'".to_string(),
            1,
            10
        )]
    );
}

/// Every shape a tag resolves through, including the ones Laravel and
/// Livewire reach by studly-casing the name rather than by the exact name
/// the class kebab-cases to.
#[tokio::test]
async fn every_resolvable_tag_is_accepted() {
    let (backend, _dir, uri) = start(
        "<x-alert />\n\
         <x-forms.date-picker />\n\
         <x-forms.datePicker />\n\
         <x-buttons.back>Back</x-buttons.back>\n\
         <x-slot:title>Title</x-slot>\n\
         <x-dynamic-component component=\"alert\" />\n\
         <x-mail::message>Hi</x-mail::message>\n\
         <livewire:counter />\n\
         <livewire:dashboard.last-24-hours-orders />\n\
         <livewire:dashboard.last24-hours-orders />\n\
         <livewire:styles />\n\
         <livewire:is :component=\"$name\" />\n",
        &[],
    )
    .await;
    assert_eq!(unknown_components(&backend, &uri), vec![]);
}

/// A package's views are only enumerable when a provider registers where
/// they are, so a tag under some other prefix is not judged.
#[tokio::test]
async fn a_tag_under_an_unregistered_prefix_is_not_judged() {
    let (backend, _dir, uri) = start("<x-flux::button />\n", &[]).await;
    assert_eq!(unknown_components(&backend, &uri), vec![]);
}

/// `Blade::component()` names a component nothing on disk does, and one
/// registered under a name the scan cannot read could be any tag at all.
#[tokio::test]
async fn provider_aliases_are_honoured() {
    let provider = "<?php\nnamespace App\\Providers;\n\
        use Illuminate\\Support\\Facades\\Blade;\n\
        class AppServiceProvider {\n\
            public function boot(): void {\n\
                Blade::component('package-alert', \\App\\View\\Components\\Alert::class);\n\
            }\n\
        }\n";
    let (backend, _dir, uri) = start(
        "<x-package-alert />\n<x-package-alret />\n",
        &[
            ("bootstrap/providers.php", PROVIDERS_PHP),
            ("app/Providers/AppServiceProvider.php", provider),
        ],
    )
    .await;
    let reported = unknown_components(&backend, &uri);
    assert_eq!(reported.len(), 1, "{reported:?}");
    assert_eq!(reported[0].2, 1, "only the misspelt alias: {reported:?}");

    let dynamic = "<?php\nnamespace App\\Providers;\n\
        use Illuminate\\Support\\Facades\\Blade;\n\
        class AppServiceProvider {\n\
            public function boot(): void {\n\
                foreach ($this->icons() as $icon) {\n\
                    Blade::component($icon, \\App\\View\\Components\\Alert::class);\n\
                }\n\
            }\n\
        }\n";
    let (backend, _dir, uri) = start(
        "<x-heroicon-o-user />\n",
        &[
            ("bootstrap/providers.php", PROVIDERS_PHP),
            ("app/Providers/AppServiceProvider.php", dynamic),
        ],
    )
    .await;
    assert_eq!(unknown_components(&backend, &uri), vec![]);
}

/// A registered prefix is enumerable, so a tag under it is judged.
#[tokio::test]
async fn a_tag_under_a_registered_anonymous_namespace_is_judged() {
    let provider = "<?php\nnamespace App\\Providers;\n\
        use Illuminate\\Support\\Facades\\Blade;\n\
        class AppServiceProvider {\n\
            public function boot(): void {\n\
                Blade::anonymousComponentNamespace('shop.components', 'shop');\n\
            }\n\
        }\n";
    let (backend, _dir, uri) = start(
        "<x-shop::card />\n<x-shop::crad />\n",
        &[
            ("bootstrap/providers.php", PROVIDERS_PHP),
            ("app/Providers/AppServiceProvider.php", provider),
            (
                "resources/views/shop/components/card.blade.php",
                "<div></div>\n",
            ),
        ],
    )
    .await;
    let reported = unknown_components(&backend, &uri);
    assert_eq!(reported.len(), 1, "{reported:?}");
    assert_eq!(reported[0].1, "Unknown Blade component: 'shop::crad'");
}

// ── Quick fixes ─────────────────────────────────────────────────────────────

async fn actions_at(backend: &Backend, uri: &Uri, line: u32, character: u32) -> Vec<CodeAction> {
    let position = Position::new(line, character);
    code_actions_via_server(backend, uri, Range::new(position, position))
        .await
        .unwrap_or_default()
        .into_iter()
        .filter_map(|action| match action {
            CodeActionResponse::CodeAction(action) => Some(action),
            CodeActionResponse::Command(_) => None,
        })
        .collect()
}

fn find<'a>(actions: &'a [CodeAction], title: &str) -> &'a CodeAction {
    actions
        .iter()
        .find(|action| action.title == title)
        .unwrap_or_else(|| {
            panic!(
                "expected {title:?}, got: {:?}",
                actions.iter().map(|a| &a.title).collect::<Vec<_>>()
            )
        })
}

/// The files an action creates, relative to the project root, with the
/// content each is given.
fn created_files(action: &CodeAction, root: &std::path::Path) -> Vec<(String, String)> {
    let operations = action
        .edit
        .as_ref()
        .and_then(|edit| edit.document_changes.as_ref())
        .expect("a create action carries document changes");
    let relative = |uri: &Uri| {
        uri.to_file_path()
            .unwrap()
            .strip_prefix(root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/")
    };
    let mut files: Vec<(String, String)> = operations
        .iter()
        .filter_map(|op| match op {
            DocumentChange::CreateFile(create) => Some((relative(&create.uri), String::new())),
            _ => None,
        })
        .collect();
    for op in operations {
        if let DocumentChange::TextDocumentEdit(edit) = op {
            let path = relative(&edit.text_document.text_document_identifier.uri);
            let text = edit
                .edits
                .iter()
                .map(|edit| match edit {
                    Edit::TextEdit(edit) => edit.new_text.clone(),
                    _ => String::new(),
                })
                .collect::<String>();
            if let Some(file) = files.iter_mut().find(|(file, _)| *file == path) {
                file.1 = text;
            }
        }
    }
    files
}

#[tokio::test]
async fn a_misspelt_tag_is_offered_the_name_it_meant_on_both_tags() {
    let template = "<x-buttons.bak>Back</x-buttons.bak>\n";
    let (backend, _dir, uri) = start(template, &[]).await;
    let actions = actions_at(&backend, &uri, 0, 5).await;
    let change = find(&actions, "Change to 'buttons.back'");
    assert_eq!(change.is_preferred, Some(true));
    let edits = &change.edit.as_ref().unwrap().changes.as_ref().unwrap()[&uri];
    let mut ranges: Vec<(u32, u32)> = edits
        .iter()
        .map(|edit| (edit.range.start.character, edit.range.end.character))
        .collect();
    ranges.sort();
    assert_eq!(
        ranges,
        vec![(3, 14), (23, 34)],
        "the opening and closing names"
    );
    assert!(edits.iter().all(|edit| edit.new_text == "buttons.back"));
}

#[tokio::test]
async fn an_unknown_blade_component_can_be_created_either_way() {
    let (backend, dir, uri) = start("<x-forms.color-picker />\n", &[]).await;
    let actions = actions_at(&backend, &uri, 0, 5).await;

    let anonymous = find(&actions, "Create anonymous component 'forms.color-picker'");
    assert_eq!(
        created_files(anonymous, dir.path()),
        vec![(
            "resources/views/components/forms/color-picker.blade.php".to_string(),
            String::new()
        )]
    );

    let class = find(&actions, "Create component class for 'forms.color-picker'");
    let files = created_files(class, dir.path());
    assert_eq!(files[0].0, "app/View/Components/Forms/ColorPicker.php");
    assert!(
        files[0]
            .1
            .contains("namespace App\\View\\Components\\Forms;")
            && files[0].1.contains("class ColorPicker extends Component")
            && files[0]
                .1
                .contains("return view('components.forms.color-picker');"),
        "{}",
        files[0].1
    );
    assert_eq!(
        files[1].0,
        "resources/views/components/forms/color-picker.blade.php"
    );
}

#[tokio::test]
async fn an_unknown_livewire_component_can_be_created() {
    let (backend, dir, uri) = start("<livewire:orders.overview />\n", &[]).await;
    let actions = actions_at(&backend, &uri, 0, 12).await;
    let create = find(&actions, "Create Livewire component 'orders.overview'");
    let files = created_files(create, dir.path());
    assert_eq!(files[0].0, "app/Livewire/Orders/Overview.php");
    assert!(
        files[0].1.contains("namespace App\\Livewire\\Orders;")
            && files[0]
                .1
                .contains("return view('livewire.orders.overview');"),
        "{}",
        files[0].1
    );
    assert_eq!(
        files[1].0,
        "resources/views/livewire/orders/overview.blade.php"
    );
}

#[tokio::test]
async fn a_known_tag_is_offered_nothing() {
    let (backend, _dir, uri) = start("<x-alert />\n", &[]).await;
    let actions = actions_at(&backend, &uri, 0, 4).await;
    assert!(
        !actions
            .iter()
            .any(|action| action.title.starts_with("Create") || action.title.starts_with("Change")),
        "{:?}",
        actions.iter().map(|a| &a.title).collect::<Vec<_>>()
    );
}
