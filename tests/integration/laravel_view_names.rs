//! View names and the variables a `view()` call hands its template.
//!
//! Cases adapted from laravel-lsp's MIT-licensed test suite.
//!
//! A view name is resolved the way Laravel's `FileViewFinder` resolves it:
//! against every configured view root, with every extension it tries, and,
//! for a `package::name` view, against the directories a service provider
//! registers through `loadViewsFrom()` (the application's published copy
//! under `resources/views/vendor/<package>` first).  The data a call site
//! passes, whichever of `view('x', [...])`, `compact()` or `->with()` spells
//! it, types the template it names.

use crate::common::{
    LARAVEL_APP_COMPOSER, blade_undefined_variables, create_psr4_workspace, definition_locations,
    find_action, get_code_actions_in_range, goto_definition_at, markup_hover_at,
    messages_with_code, open_blade_template, open_php_file, position_of, workspace_path,
};
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

const ITEM_CLASS: &str =
    "<?php\nnamespace App;\nclass Item { public string $name = ''; public int $price = 0; }\n";

const COUPON_CLASS: &str = "<?php\nnamespace App;\nclass Coupon { public string $code = ''; }\n";

/// An application that also carries a package under `packages/widgets`,
/// autoloaded from the root `composer.json` the way a path package is.
const PACKAGE_COMPOSER: &str = r#"{
    "require": { "laravel/framework": "^11.0" },
    "autoload": { "psr-4": { "App\\": "app/", "Acme\\Widgets\\": "packages/widgets/src/" } }
}"#;

const PACKAGE_PROVIDER_LIST: &str =
    "<?php\nreturn [\n    Acme\\Widgets\\WidgetsServiceProvider::class,\n];\n";

/// A package provider registering its templates under the `widgets`
/// namespace.
const PACKAGE_VIEW_PROVIDER: &str = "\
<?php
namespace Acme\\Widgets;

class WidgetsServiceProvider
{
    public function boot(): void
    {
        $this->loadViewsFrom(__DIR__.'/../resources/views', 'widgets');
    }
}
";

/// A controller class in `app/` whose one method runs `body`.
fn controller(class: &str, body: &str) -> String {
    format!(
        "<?php\nnamespace App;\nclass {class} {{\n    public function show(): mixed {{\n        {body}\n    }}\n}}\n"
    )
}

/// Open every PHP caller in `callers`, then the template at `template`,
/// the order an editor that has the controller open already would use.
async fn open_callers_then_template(
    backend: &phpantom_lsp::Backend,
    callers: &[&str],
    template: &str,
) -> Url {
    for caller in callers {
        open_php_file(backend, caller).await;
    }
    open_blade_template(backend, template).await
}

/// The go-to-definition targets at the first occurrence of `needle` in
/// `content`, a couple of characters into it.
async fn definitions_at(
    backend: &phpantom_lsp::Backend,
    uri: &Url,
    content: &str,
    needle: &str,
) -> Vec<Location> {
    let position = position_of(content, needle);
    definition_locations(
        goto_definition_at(backend, uri, position.line, position.character + 2).await,
    )
}

/// The `invalid_laravel_view` messages the slow pass reports on `uri`.
fn view_diagnostics(backend: &phpantom_lsp::Backend, uri: &Url, content: &str) -> Vec<String> {
    let mut diags = Vec::new();
    backend.collect_slow_diagnostics(uri.as_str(), content, &mut diags);
    messages_with_code(&diags, "invalid_laravel_view")
}

// ─── Resolving a view name ──────────────────────────────────────────────────

/// `FileViewFinder` tries `php` right after `blade.php`, so a plain PHP
/// template is as much a view as a Blade one.
#[tokio::test]
async fn a_plain_php_template_is_a_view() {
    let caller = controller("LegacyController", "return view('legacy.report');");
    let (backend, _dir) = create_psr4_workspace(
        LARAVEL_APP_COMPOSER,
        &[
            ("app/LegacyController.php", &caller),
            (
                "resources/views/legacy/report.php",
                "<p><?= $title ?></p>\n",
            ),
        ],
    );
    backend.initialized(InitializedParams {}).await;
    let uri = open_php_file(&backend, "app/LegacyController.php").await;

    assert!(
        view_diagnostics(&backend, &uri, &caller).is_empty(),
        "a .php template should satisfy the view name"
    );
    let found = definitions_at(&backend, &uri, &caller, "legacy.report").await;
    assert!(
        found.iter().any(|l| l
            .uri
            .as_str()
            .ends_with("/resources/views/legacy/report.php")),
        "go-to-definition should reach the plain PHP template, got {found:?}"
    );
}

/// A `package::name` view resolves inside the directory the package's
/// provider registered for that namespace, and a name the package does not
/// ship is still reported.
#[tokio::test]
async fn a_package_view_resolves_through_its_registered_namespace() {
    let caller = controller(
        "WidgetController",
        "view('widgets::card');\n        return view('widgets::missing');",
    );
    let (backend, _dir) = create_psr4_workspace(
        PACKAGE_COMPOSER,
        &[
            ("bootstrap/providers.php", PACKAGE_PROVIDER_LIST),
            (
                "packages/widgets/src/WidgetsServiceProvider.php",
                PACKAGE_VIEW_PROVIDER,
            ),
            (
                "packages/widgets/resources/views/card.blade.php",
                "<div class=\"card\"></div>\n",
            ),
            ("app/WidgetController.php", &caller),
        ],
    );
    backend.initialized(InitializedParams {}).await;
    let uri = open_php_file(&backend, "app/WidgetController.php").await;

    let messages = view_diagnostics(&backend, &uri, &caller);
    assert_eq!(
        messages.len(),
        1,
        "only the name the package lacks is unknown, got {messages:?}"
    );
    assert!(messages[0].contains("widgets::missing"), "got {messages:?}");

    let found = definitions_at(&backend, &uri, &caller, "widgets::card").await;
    assert!(
        found.iter().any(
            |l| l.uri.as_str().ends_with("/resources/views/card.blade.php")
                && l.uri.as_str().contains("/packages/widgets/")
        ),
        "go-to-definition should reach the package's template, got {found:?}"
    );
}

/// `loadViewsFrom()` registers `resources/views/vendor/<namespace>` ahead
/// of the package's own directory, so a published copy is the template
/// Laravel renders.
#[tokio::test]
#[ignore = "known gap: published package views under resources/views/vendor are ignored"]
async fn a_published_copy_of_a_package_view_wins_over_the_original() {
    let caller = controller("WidgetController", "return view('widgets::card');");
    let (backend, _dir) = create_psr4_workspace(
        PACKAGE_COMPOSER,
        &[
            ("bootstrap/providers.php", PACKAGE_PROVIDER_LIST),
            (
                "packages/widgets/src/WidgetsServiceProvider.php",
                PACKAGE_VIEW_PROVIDER,
            ),
            (
                "packages/widgets/resources/views/card.blade.php",
                "<div class=\"card\"></div>\n",
            ),
            (
                "resources/views/vendor/widgets/card.blade.php",
                "<div class=\"card card--custom\"></div>\n",
            ),
            ("app/WidgetController.php", &caller),
        ],
    );
    backend.initialized(InitializedParams {}).await;
    let uri = open_php_file(&backend, "app/WidgetController.php").await;

    let found = definitions_at(&backend, &uri, &caller, "widgets::card").await;
    let first = found.first().expect("the view should resolve");
    assert!(
        first
            .uri
            .as_str()
            .ends_with("/resources/views/vendor/widgets/card.blade.php"),
        "the published copy is what renders, so it should be the first target, got {found:?}"
    );
}

/// A template that exists only in the published directory is still a
/// `package::name` view: the finder looks there before the package.
#[tokio::test]
#[ignore = "known gap: published package views under resources/views/vendor are ignored"]
async fn a_view_only_the_published_directory_holds_is_known() {
    let caller = controller("WidgetController", "return view('widgets::banner');");
    let (backend, _dir) = create_psr4_workspace(
        PACKAGE_COMPOSER,
        &[
            ("bootstrap/providers.php", PACKAGE_PROVIDER_LIST),
            (
                "packages/widgets/src/WidgetsServiceProvider.php",
                PACKAGE_VIEW_PROVIDER,
            ),
            (
                "packages/widgets/resources/views/card.blade.php",
                "<div class=\"card\"></div>\n",
            ),
            (
                "resources/views/vendor/widgets/banner.blade.php",
                "<div class=\"banner\"></div>\n",
            ),
            ("app/WidgetController.php", &caller),
        ],
    );
    backend.initialized(InitializedParams {}).await;
    let uri = open_php_file(&backend, "app/WidgetController.php").await;

    let messages = view_diagnostics(&backend, &uri, &caller);
    assert!(
        messages.is_empty(),
        "widgets::banner resolves through the published directory, got {messages:?}"
    );
}

// ─── Creating a missing view ────────────────────────────────────────────────

/// The file a "Create missing view" action on `line` of `content` would
/// create.
fn created_view_path(
    backend: &phpantom_lsp::Backend,
    uri: &Url,
    content: &str,
    line: u32,
) -> std::path::PathBuf {
    let actions = get_code_actions_in_range(
        backend,
        uri.as_str(),
        content,
        Range::new(Position::new(line, 0), Position::new(line, 200)),
    );
    let action = find_action(&actions, "Create missing view").expect("should offer the action");
    let edit = action.edit.as_ref().expect("action should carry an edit");
    let Some(DocumentChanges::Operations(ops)) = edit.document_changes.as_ref() else {
        panic!("expected resource operations, got {edit:?}");
    };
    match &ops[0] {
        DocumentChangeOperation::Op(ResourceOp::Create(create)) => {
            create.uri.to_file_path().unwrap()
        }
        other => panic!("expected a CreateFile operation, got {other:?}"),
    }
}

/// Laravel treats `/` like `.` and ignores a leading separator, so a
/// slash-spelled missing view is created where the dotted name points.
#[test]
fn a_slash_spelled_missing_view_is_created_under_its_directories() {
    let caller = controller("ReportController", "return view('/reports/monthly');");
    let (backend, dir) = create_psr4_workspace(
        LARAVEL_APP_COMPOSER,
        &[
            ("app/ReportController.php", &caller),
            ("resources/views/welcome.blade.php", "<p>hi</p>\n"),
        ],
    );
    let uri = Url::from_file_path(dir.path().join("app/ReportController.php")).unwrap();
    backend.update_ast(uri.as_str(), &caller);

    let path = created_view_path(&backend, &uri, &caller, 4);
    assert_eq!(
        path,
        dir.path().join("resources/views/reports/monthly.blade.php")
    );
}

/// A missing `package::name` view belongs in the directory its package
/// registered, not in the application's view root.
#[tokio::test]
async fn a_missing_package_view_is_created_in_the_packages_directory() {
    let caller = controller("WidgetController", "return view('widgets::badge');");
    let (backend, _dir) = create_psr4_workspace(
        PACKAGE_COMPOSER,
        &[
            ("bootstrap/providers.php", PACKAGE_PROVIDER_LIST),
            (
                "packages/widgets/src/WidgetsServiceProvider.php",
                PACKAGE_VIEW_PROVIDER,
            ),
            (
                "packages/widgets/resources/views/card.blade.php",
                "<div class=\"card\"></div>\n",
            ),
            ("app/WidgetController.php", &caller),
        ],
    );
    backend.initialized(InitializedParams {}).await;
    let uri = open_php_file(&backend, "app/WidgetController.php").await;

    let path = created_view_path(&backend, &uri, &caller, 4);
    let path = path.to_string_lossy();
    assert!(
        path.contains("/packages/widgets/") && path.ends_with("/resources/views/badge.blade.php"),
        "the template should be created in the package's view directory, got {path}"
    );
}

// ─── Data a call site passes ────────────────────────────────────────────────

/// `->with([...])` merges a whole array into the view's data, the same as
/// the data argument does.
#[tokio::test]
async fn an_array_passed_to_with_types_the_template_variables() {
    let (backend, _dir) = create_psr4_workspace(
        LARAVEL_APP_COMPOSER,
        &[
            ("app/Item.php", ITEM_CLASS),
            (
                "app/ShopController.php",
                &controller(
                    "ShopController",
                    "return view('shop')->with(['item' => new Item()]);",
                ),
            ),
            ("resources/views/shop.blade.php", "{{ $item->name }}\n"),
        ],
    );
    let blade = open_callers_then_template(
        &backend,
        &["app/ShopController.php"],
        "resources/views/shop.blade.php",
    )
    .await;

    let hover = markup_hover_at(&backend, &blade, 0, 4).await;
    assert!(hover.contains("Item"), "got {hover}");
}

/// `compact()` reads the named locals, a typed parameter included.
#[tokio::test]
async fn compact_of_a_typed_parameter_types_the_template_variable() {
    let caller = "<?php\nnamespace App;\nclass ShopController {\n    public function show(Item $item): mixed {\n        return view('shop', compact('item'));\n    }\n}\n";
    let (backend, _dir) = create_psr4_workspace(
        LARAVEL_APP_COMPOSER,
        &[
            ("app/Item.php", ITEM_CLASS),
            ("app/ShopController.php", caller),
            ("resources/views/shop.blade.php", "{{ $item->name }}\n"),
        ],
    );
    let blade = open_callers_then_template(
        &backend,
        &["app/ShopController.php"],
        "resources/views/shop.blade.php",
    )
    .await;

    let hover = markup_hover_at(&backend, &blade, 0, 4).await;
    assert!(hover.contains("Item"), "got {hover}");
}

/// A value whose type cannot be worked out still puts its name in scope:
/// the template receives it, whatever it is.
#[tokio::test]
async fn a_passed_variable_of_unknown_type_is_still_defined() {
    let caller = "<?php\nnamespace App;\nclass ShopController {\n    public function show($mystery): mixed {\n        return view('shop', ['thing' => $mystery]);\n    }\n}\n";
    let (backend, _dir) = create_psr4_workspace(
        LARAVEL_APP_COMPOSER,
        &[
            ("app/ShopController.php", caller),
            (
                "resources/views/shop.blade.php",
                "{{ $thing }}\n{{ $other }}\n",
            ),
        ],
    );
    let blade = open_callers_then_template(
        &backend,
        &["app/ShopController.php"],
        "resources/views/shop.blade.php",
    )
    .await;

    let undefined = blade_undefined_variables(&backend, &blade);
    assert!(
        !undefined.iter().any(|m| m.contains("$thing")),
        "$thing is passed, so it is defined: {undefined:?}"
    );
    assert!(
        undefined.iter().any(|m| m.contains("$other")),
        "a variable nobody passes is still undefined: {undefined:?}"
    );
}

/// Two methods of one controller rendering the same view are two call
/// sites, and the template sees what either of them passes.
#[tokio::test]
async fn two_call_sites_in_one_file_union_their_types() {
    let caller = "<?php\nnamespace App;\nclass ShopController {\n    public function item(): mixed {\n        return view('shop', ['subject' => new Item()]);\n    }\n    public function coupon(): mixed {\n        return view('shop', ['subject' => new Coupon()]);\n    }\n}\n";
    let (backend, _dir) = create_psr4_workspace(
        LARAVEL_APP_COMPOSER,
        &[
            ("app/Item.php", ITEM_CLASS),
            ("app/Coupon.php", COUPON_CLASS),
            ("app/ShopController.php", caller),
            ("resources/views/shop.blade.php", "{{ $subject }}\n"),
        ],
    );
    let blade = open_callers_then_template(
        &backend,
        &["app/ShopController.php"],
        "resources/views/shop.blade.php",
    )
    .await;

    let hover = markup_hover_at(&backend, &blade, 0, 4).await;
    assert!(
        hover.contains("Item") && hover.contains("Coupon"),
        "both call sites should contribute, got {hover}"
    );
}

/// A key written twice in one array literal keeps the last value, as PHP
/// does.
#[tokio::test]
async fn a_duplicate_key_in_the_data_array_keeps_the_last_value() {
    let (backend, _dir) = create_psr4_workspace(
        LARAVEL_APP_COMPOSER,
        &[
            ("app/Item.php", ITEM_CLASS),
            ("app/Coupon.php", COUPON_CLASS),
            (
                "app/ShopController.php",
                &controller(
                    "ShopController",
                    "return view('shop', ['subject' => new Item(), 'subject' => new Coupon()]);",
                ),
            ),
            ("resources/views/shop.blade.php", "{{ $subject }}\n"),
        ],
    );
    let blade = open_callers_then_template(
        &backend,
        &["app/ShopController.php"],
        "resources/views/shop.blade.php",
    )
    .await;

    let hover = markup_hover_at(&backend, &blade, 0, 4).await;
    assert!(
        hover.contains("Coupon") && !hover.contains("Item"),
        "the later entry replaces the earlier one, got {hover}"
    );
}

/// `View::with()` assigns into the data the view was made with, so a key it
/// names replaces the one the data argument passed.
#[tokio::test]
async fn a_with_call_replaces_the_same_key_from_the_data_argument() {
    let (backend, _dir) = create_psr4_workspace(
        LARAVEL_APP_COMPOSER,
        &[
            ("app/Item.php", ITEM_CLASS),
            ("app/Coupon.php", COUPON_CLASS),
            (
                "app/ShopController.php",
                &controller(
                    "ShopController",
                    "return view('shop', ['subject' => new Item()])->with('subject', new Coupon());",
                ),
            ),
            ("resources/views/shop.blade.php", "{{ $subject }}\n"),
        ],
    );
    let blade = open_callers_then_template(
        &backend,
        &["app/ShopController.php"],
        "resources/views/shop.blade.php",
    )
    .await;

    let hover = markup_hover_at(&backend, &blade, 0, 4).await;
    assert!(
        hover.contains("Coupon") && !hover.contains("Item"),
        "->with() overwrites the data argument's entry, got {hover}"
    );
}

// ─── Which template a call site feeds ───────────────────────────────────────

/// A dotted name reaches the template in the matching subdirectory.
#[tokio::test]
async fn a_nested_view_name_types_the_template_in_its_subdirectory() {
    let (backend, _dir) = create_psr4_workspace(
        LARAVEL_APP_COMPOSER,
        &[
            ("app/Item.php", ITEM_CLASS),
            (
                "app/ShopController.php",
                &controller(
                    "ShopController",
                    "return view('shop.items.show', ['item' => new Item()]);",
                ),
            ),
            (
                "resources/views/shop/items/show.blade.php",
                "{{ $item->name }}\n",
            ),
        ],
    );
    let blade = open_callers_then_template(
        &backend,
        &["app/ShopController.php"],
        "resources/views/shop/items/show.blade.php",
    )
    .await;

    let hover = markup_hover_at(&backend, &blade, 0, 4).await;
    assert!(hover.contains("Item"), "got {hover}");
}

/// A slash-spelled name is the same view as the dotted one, so its data
/// reaches the same template.
#[tokio::test]
async fn a_slash_spelled_view_name_types_the_same_template() {
    let (backend, _dir) = create_psr4_workspace(
        LARAVEL_APP_COMPOSER,
        &[
            ("app/Item.php", ITEM_CLASS),
            (
                "app/ShopController.php",
                &controller(
                    "ShopController",
                    "return view('shop/items/show', ['item' => new Item()]);",
                ),
            ),
            (
                "resources/views/shop/items/show.blade.php",
                "{{ $item->name }}\n",
            ),
        ],
    );
    let blade = open_callers_then_template(
        &backend,
        &["app/ShopController.php"],
        "resources/views/shop/items/show.blade.php",
    )
    .await;

    let hover = markup_hover_at(&backend, &blade, 0, 4).await;
    assert!(hover.contains("Item"), "got {hover}");
}

/// A template under a root `config/view.php` adds is addressed by the same
/// bare name, and receives that name's data.
#[tokio::test]
async fn a_template_under_a_configured_view_root_is_typed_by_its_call_site() {
    let view_config = "<?php\nreturn [\n    'paths' => [\n        base_path('resources/backoffice/views'),\n        resource_path('views'),\n    ],\n];\n";
    let (backend, _dir) = create_psr4_workspace(
        LARAVEL_APP_COMPOSER,
        &[
            ("config/view.php", view_config),
            ("app/Item.php", ITEM_CLASS),
            (
                "app/PanelController.php",
                &controller(
                    "PanelController",
                    "return view('panel', ['item' => new Item()]);",
                ),
            ),
            (
                "resources/backoffice/views/panel.blade.php",
                "{{ $item->name }}\n",
            ),
        ],
    );
    let blade = open_callers_then_template(
        &backend,
        &["app/PanelController.php"],
        "resources/backoffice/views/panel.blade.php",
    )
    .await;

    let hover = markup_hover_at(&backend, &blade, 0, 4).await;
    assert!(hover.contains("Item"), "got {hover}");
}

/// A package's own template is fed by the `package::name` calls that
/// render it.
#[tokio::test]
async fn a_package_template_is_typed_by_its_namespaced_call_site() {
    let (backend, _dir) = create_psr4_workspace(
        PACKAGE_COMPOSER,
        &[
            ("bootstrap/providers.php", PACKAGE_PROVIDER_LIST),
            (
                "packages/widgets/src/WidgetsServiceProvider.php",
                PACKAGE_VIEW_PROVIDER,
            ),
            ("app/Item.php", ITEM_CLASS),
            (
                "app/WidgetController.php",
                &controller(
                    "WidgetController",
                    "return view('widgets::card', ['item' => new Item()]);",
                ),
            ),
            (
                "packages/widgets/resources/views/card.blade.php",
                "{{ $item->name }}\n",
            ),
        ],
    );
    backend.initialized(InitializedParams {}).await;
    let blade = open_callers_then_template(
        &backend,
        &["app/WidgetController.php"],
        "packages/widgets/resources/views/card.blade.php",
    )
    .await;

    let hover = markup_hover_at(&backend, &blade, 0, 4).await;
    assert!(hover.contains("Item"), "got {hover}");
}

/// The published copy of a package template is what a `package::name` call
/// renders, so that call's data types it.
#[tokio::test]
#[ignore = "known gap: published package views under resources/views/vendor are ignored"]
async fn a_published_package_template_is_typed_by_its_namespaced_call_site() {
    let (backend, _dir) = create_psr4_workspace(
        PACKAGE_COMPOSER,
        &[
            ("bootstrap/providers.php", PACKAGE_PROVIDER_LIST),
            (
                "packages/widgets/src/WidgetsServiceProvider.php",
                PACKAGE_VIEW_PROVIDER,
            ),
            ("app/Item.php", ITEM_CLASS),
            (
                "app/WidgetController.php",
                &controller(
                    "WidgetController",
                    "return view('widgets::card', ['item' => new Item()]);",
                ),
            ),
            (
                "packages/widgets/resources/views/card.blade.php",
                "<div class=\"card\"></div>\n",
            ),
            (
                "resources/views/vendor/widgets/card.blade.php",
                "{{ $item->name }}\n",
            ),
        ],
    );
    backend.initialized(InitializedParams {}).await;
    let blade = open_callers_then_template(
        &backend,
        &["app/WidgetController.php"],
        "resources/views/vendor/widgets/card.blade.php",
    )
    .await;

    let hover = markup_hover_at(&backend, &blade, 0, 4).await;
    assert!(hover.contains("Item"), "got {hover}");
}

// ─── Keeping up with edits ──────────────────────────────────────────────────

/// Replace a caller's text on disk and in the editor, the way saving an
/// edit does.
async fn rewrite_caller(backend: &phpantom_lsp::Backend, relative: &str, content: &str) {
    std::fs::write(workspace_path(backend, relative), content).unwrap();
    open_php_file(backend, relative).await;
}

/// What a caller passed before an edit is gone after it: the template
/// follows the call site's current data, not the first one it saw.
#[tokio::test]
async fn editing_the_call_site_retypes_the_template() {
    let (backend, _dir) = create_psr4_workspace(
        LARAVEL_APP_COMPOSER,
        &[
            ("app/Item.php", ITEM_CLASS),
            ("app/Coupon.php", COUPON_CLASS),
            (
                "app/ShopController.php",
                &controller(
                    "ShopController",
                    "return view('shop', ['subject' => new Item()]);",
                ),
            ),
            ("resources/views/shop.blade.php", "{{ $subject }}\n"),
        ],
    );
    let blade = open_callers_then_template(
        &backend,
        &["app/ShopController.php"],
        "resources/views/shop.blade.php",
    )
    .await;
    let before = markup_hover_at(&backend, &blade, 0, 4).await;
    assert!(before.contains("Item"), "got {before}");

    rewrite_caller(
        &backend,
        "app/ShopController.php",
        &controller(
            "ShopController",
            "return view('shop', ['subject' => new Coupon()]);",
        ),
    )
    .await;
    open_blade_template(&backend, "resources/views/shop.blade.php").await;

    let after = markup_hover_at(&backend, &blade, 0, 4).await;
    assert!(
        after.contains("Coupon") && !after.contains("Item"),
        "the edited call site's type should replace the old one, got {after}"
    );
}

/// One caller that stops rendering a template takes only its own data
/// with it; another caller's contribution stays.
#[tokio::test]
async fn a_caller_that_stops_rendering_leaves_the_other_callers_data() {
    let (backend, _dir) = create_psr4_workspace(
        LARAVEL_APP_COMPOSER,
        &[
            ("app/Item.php", ITEM_CLASS),
            ("app/Coupon.php", COUPON_CLASS),
            (
                "app/ItemController.php",
                &controller(
                    "ItemController",
                    "return view('shop', ['subject' => new Item()]);",
                ),
            ),
            (
                "app/CouponController.php",
                &controller(
                    "CouponController",
                    "return view('shop', ['subject' => new Coupon()]);",
                ),
            ),
            ("resources/views/shop.blade.php", "{{ $subject }}\n"),
            ("resources/views/elsewhere.blade.php", "\n"),
        ],
    );
    let blade = open_callers_then_template(
        &backend,
        &["app/ItemController.php", "app/CouponController.php"],
        "resources/views/shop.blade.php",
    )
    .await;
    let before = markup_hover_at(&backend, &blade, 0, 4).await;
    assert!(
        before.contains("Item") && before.contains("Coupon"),
        "got {before}"
    );

    rewrite_caller(
        &backend,
        "app/ItemController.php",
        &controller(
            "ItemController",
            "return view('elsewhere', ['subject' => new Item()]);",
        ),
    )
    .await;
    open_blade_template(&backend, "resources/views/shop.blade.php").await;

    let after = markup_hover_at(&backend, &blade, 0, 4).await;
    assert!(
        after.contains("Coupon") && !after.contains("Item"),
        "only the caller that moved away should drop out, got {after}"
    );
}
