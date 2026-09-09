//! The call sites beyond the leading helper that name a Laravel string key.
//!
//! `route('x')` is only one of the ways a route name is written: the signed
//! URL helpers, the redirect facades, and the "is the current route named …?"
//! checks all name one too, and a name is only navigable where the indexer
//! recognises the call. The same goes for a translation key behind
//! `hasForLocale()`, a config key inside `getMany()`, and an environment
//! variable behind `Env::get()`.

use crate::common::{create_psr4_workspace, open_php};
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

const COMPOSER: &str = r#"{
    "require": { "laravel/framework": "^11.0" },
    "autoload": { "psr-4": { "App\\": "app/" } }
}"#;

/// The Markdown a hover at `line`/`character` of an already-open `uri` holds.
async fn hover_text(
    backend: &phpantom_lsp::Backend,
    uri: &Url,
    line: u32,
    character: u32,
) -> String {
    let hover = backend
        .hover(HoverParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position: Position { line, character },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
        })
        .await
        .unwrap()
        .expect("the position should hover");
    match hover.contents {
        HoverContents::Markup(markup) => markup.value,
        other => panic!("expected markup hover, got {other:?}"),
    }
}

/// Every location go-to-definition offers at `line`/`character` of `relative`.
async fn definitions(
    backend: &phpantom_lsp::Backend,
    dir: &tempfile::TempDir,
    relative: &str,
    line: u32,
    character: u32,
) -> Vec<Location> {
    backend.initialized(InitializedParams {}).await;
    let path = dir.path().join(relative);
    let text = std::fs::read_to_string(&path).unwrap();
    let uri = Url::from_file_path(&path).unwrap();
    open_php(backend, &uri, &text).await;

    let response = backend
        .goto_definition(GotoDefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position: Position { line, character },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await
        .unwrap();
    match response {
        Some(GotoDefinitionResponse::Scalar(location)) => vec![location],
        Some(GotoDefinitionResponse::Array(locations)) => locations,
        Some(GotoDefinitionResponse::Link(links)) => links
            .into_iter()
            .map(|link| Location {
                uri: link.target_uri,
                range: link.target_range,
            })
            .collect(),
        None => Vec::new(),
    }
}

/// A workspace whose `routes/web.php` names one route per demo.
fn route_workspace(caller: &str) -> (phpantom_lsp::Backend, tempfile::TempDir) {
    let routes = "<?php\n\
        Route::get('/orders/{order}', 'show')->name('orders.show');\n\
        Route::get('/admin/users', 'index')->name('admin.users.index');\n";
    create_psr4_workspace(
        COMPOSER,
        &[("routes/web.php", routes), ("app/Demo.php", caller)],
    )
}

/// The signed-URL and redirect helpers name a route the way `route()` does,
/// so go-to-definition reaches the registration from any of them.
#[tokio::test]
async fn the_signed_url_and_redirect_helpers_reach_the_route() {
    for call in [
        "URL::signedRoute('orders.show', ['order' => 1])",
        "Redirect::route('orders.show')",
        "redirect()->route('orders.show')",
        "response()->redirectToRoute('orders.show')",
        "url()->temporarySignedRoute('orders.show', 60)",
    ] {
        let caller = format!(
            "<?php\nnamespace App;\nclass Demo {{\n\
             \x20   public function go() {{\n\
             \x20       return {call};\n\
             \x20   }}\n}}\n"
        );
        let (backend, dir) = route_workspace(&caller);
        let character = caller
            .lines()
            .nth(4)
            .and_then(|line| line.find("orders.show"))
            .expect("the demo names a route") as u32
            + 2;

        let found = definitions(&backend, &dir, "app/Demo.php", 4, character).await;
        assert!(
            found
                .iter()
                .any(|location| location.uri.as_str().ends_with("/routes/web.php")),
            "`{call}` should reach the route registration, got {found:?}"
        );
    }
}

/// A `Route::is('admin.*')` check names a pattern, and every route under it
/// is somewhere the check can land.
#[tokio::test]
async fn a_route_pattern_reaches_the_routes_it_matches() {
    let caller = "<?php\nnamespace App;\nclass Demo {\n\
        \x20   public function go(): bool {\n\
        \x20       return Route::is('admin.*');\n\
        \x20   }\n}\n";
    let (backend, dir) = route_workspace(caller);

    let found = definitions(&backend, &dir, "app/Demo.php", 4, 30).await;
    assert!(
        found
            .iter()
            .any(|location| location.uri.as_str().ends_with("/routes/web.php")),
        "a glob pattern should reach the routes it matches, got {found:?}"
    );
}

/// The unknown-route diagnostic reads a pattern as a pattern: one that
/// covers a registered route is fine, and one that covers none is the same
/// mistake a misspelled name is.
#[tokio::test]
async fn a_route_pattern_is_judged_by_what_it_matches() {
    let caller = "<?php\nnamespace App;\nclass Demo {\n\
        \x20   public function go(): void {\n\
        \x20       Route::is('admin.*');\n\
        \x20       Route::is('adnim.*');\n\
        \x20   }\n}\n";
    let (backend, dir) = route_workspace(caller);
    backend.initialized(InitializedParams {}).await;
    let uri = Url::from_file_path(dir.path().join("app/Demo.php")).unwrap();
    open_php(&backend, &uri, caller).await;

    let mut diagnostics = Vec::new();
    backend.collect_slow_diagnostics(uri.as_str(), caller, &mut diagnostics);
    let route_diagnostics: Vec<&Diagnostic> = diagnostics
        .iter()
        .filter(|diagnostic| {
            matches!(&diagnostic.code, Some(NumberOrString::String(code))
                if code == "invalid_laravel_route")
        })
        .collect();

    assert_eq!(
        route_diagnostics.len(),
        1,
        "only the pattern matching nothing should be reported, got {route_diagnostics:?}"
    );
    assert_eq!(route_diagnostics[0].range.start.line, 5);
}

/// A form request's `#[RedirectToRoute]` names the route a failed validation
/// bounces back to.
#[tokio::test]
async fn a_redirect_to_route_attribute_reaches_the_route() {
    let request = "<?php\nnamespace App;\n\
        use Illuminate\\Foundation\\Http\\Attributes\\RedirectToRoute;\n\
        #[RedirectToRoute('orders.show')]\n\
        class StoreOrderRequest {}\n";
    let (backend, dir) = route_workspace(request);

    let found = definitions(&backend, &dir, "app/Demo.php", 3, 22).await;
    assert!(
        found
            .iter()
            .any(|location| location.uri.as_str().ends_with("/routes/web.php")),
        "the attribute should reach the route registration, got {found:?}"
    );
}

/// `hasForLocale()` asks about the same keys `Lang::get()` reads.
#[tokio::test]
async fn has_for_locale_reaches_the_translation() {
    let caller = "<?php\nnamespace App;\nclass Demo {\n\
        \x20   public function go(): bool {\n\
        \x20       return Lang::hasForLocale('messages.welcome', 'da');\n\
        \x20   }\n}\n";
    let messages = "<?php\nreturn ['welcome' => 'Welcome'];\n";
    let (backend, dir) = create_psr4_workspace(
        COMPOSER,
        &[("app/Demo.php", caller), ("lang/en/messages.php", messages)],
    );

    let found = definitions(&backend, &dir, "app/Demo.php", 4, 40).await;
    assert!(
        found
            .iter()
            .any(|location| location.uri.as_str().ends_with("/lang/en/messages.php")),
        "hasForLocale should reach the translation file, got {found:?}"
    );
}

/// `getMany()` names as many config keys as its array holds, in both of the
/// spellings the repository reads it in.
#[tokio::test]
async fn get_many_reaches_each_config_key_it_lists() {
    let caller = "<?php\nnamespace App;\nclass Demo {\n\
        \x20   public function go(): array {\n\
        \x20       return Config::getMany(['app.name', 'app.timezone' => 'UTC']);\n\
        \x20   }\n}\n";
    let config = "<?php\nreturn ['name' => 'Acme', 'timezone' => 'UTC'];\n";
    let (backend, dir) = create_psr4_workspace(
        COMPOSER,
        &[("app/Demo.php", caller), ("config/app.php", config)],
    );

    for key in ["app.name", "app.timezone"] {
        let character = caller.lines().nth(4).unwrap().find(key).unwrap() as u32 + 2;
        let found = definitions(&backend, &dir, "app/Demo.php", 4, character).await;
        assert!(
            found
                .iter()
                .any(|location| location.uri.as_str().ends_with("/config/app.php")),
            "`{key}` should reach the config file, got {found:?}"
        );
    }
}

/// An environment variable is indexed like every other string key, so
/// find-references gathers every read of it across the project — including
/// the `config/*.php` files that are usually the only place it is read.
#[tokio::test]
async fn find_references_gathers_every_read_of_an_environment_variable() {
    let caller = "<?php\nnamespace App;\nclass Demo {\n\
        \x20   public function go(): string {\n\
        \x20       return env('MAIL_MAILER', 'log');\n\
        \x20   }\n}\n";
    let config = "<?php\nreturn ['default' => env('MAIL_MAILER', 'smtp')];\n";
    let (backend, dir) = create_psr4_workspace(
        COMPOSER,
        &[
            ("app/Demo.php", caller),
            ("config/mail.php", config),
            (".env", "APP_NAME=Acme\nMAIL_MAILER=log\n"),
        ],
    );
    backend.initialized(InitializedParams {}).await;
    for relative in ["app/Demo.php", "config/mail.php"] {
        let path = dir.path().join(relative);
        let text = std::fs::read_to_string(&path).unwrap();
        open_php(&backend, &Url::from_file_path(&path).unwrap(), &text).await;
    }

    let uri = Url::from_file_path(dir.path().join("app/Demo.php")).unwrap();
    let found = backend
        .references(ReferenceParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position: Position::new(4, 24),
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: ReferenceContext {
                include_declaration: true,
            },
        })
        .await
        .unwrap()
        .expect("an env key should have references");

    for expected in ["/app/Demo.php", "/config/mail.php", "/.env"] {
        assert!(
            found
                .iter()
                .any(|location| location.uri.as_str().ends_with(expected)),
            "expected a reference in {expected}, got {found:?}"
        );
    }
}

/// A hover over an environment variable says what it is set to, the way
/// every other string-key hover says what its key resolves to.  A name that
/// reads as naming a credential is the exception.
#[tokio::test]
async fn env_hover_shows_the_value_unless_the_name_reads_as_a_secret() {
    let caller = "<?php\nnamespace App;\nclass Demo {\n\
        \x20   public function go(): void {\n\
        \x20       env('MAIL_MAILER');\n\
        \x20       env('STRIPE_SECRET');\n\
        \x20       env('MAIL_FROM');\n\
        \x20   }\n}\n";
    let (backend, dir) = create_psr4_workspace(
        COMPOSER,
        &[
            ("app/Demo.php", caller),
            (
                ".env",
                "MAIL_MAILER=log  # the mailer\nSTRIPE_SECRET=sk_test_123\nMAIL_FROM=\n",
            ),
        ],
    );
    backend.initialized(InitializedParams {}).await;
    let uri = Url::from_file_path(dir.path().join("app/Demo.php")).unwrap();
    open_php(&backend, &uri, caller).await;

    let declared = hover_text(&backend, &uri, 4, 16).await;
    assert!(
        declared.contains("`log`") && declared.contains("Declared in `.env`"),
        "got {declared}"
    );

    let secret = hover_text(&backend, &uri, 5, 16).await;
    assert!(
        !secret.contains("sk_test_123") && secret.contains("Value hidden"),
        "got {secret}"
    );

    let empty = hover_text(&backend, &uri, 6, 16).await;
    assert!(empty.contains("Set to an empty value"), "got {empty}");
}

/// Completion inside `env('')` offers the project's variables — the helper
/// is the common spelling, and `Env::get('')` already completed.
#[tokio::test]
async fn the_env_helper_completes_the_projects_variables() {
    let caller = "<?php\nnamespace App;\nclass Demo {\n\
        \x20   public function go(): void {\n\
        \x20       env('');\n\
        \x20   }\n}\n";
    let (backend, dir) = create_psr4_workspace(
        COMPOSER,
        &[
            ("app/Demo.php", caller),
            (".env", "APP_NAME=Acme\nMAIL_MAILER=log\n"),
        ],
    );
    backend.initialized(InitializedParams {}).await;
    let uri = Url::from_file_path(dir.path().join("app/Demo.php")).unwrap();
    open_php(&backend, &uri, caller).await;

    let items = match backend
        .completion(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position: Position::new(4, 13),
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap()
    {
        Some(CompletionResponse::Array(items)) => items,
        Some(CompletionResponse::List(list)) => list.items,
        None => Vec::new(),
    };

    let labels: Vec<&str> = items.iter().map(|item| item.label.as_str()).collect();
    for expected in ["APP_NAME", "MAIL_MAILER"] {
        assert!(labels.contains(&expected), "got {labels:?}");
    }
}

/// A hover over a translation key shows the line it resolves to, which is
/// the one thing the reader cannot already see.
#[tokio::test]
async fn translation_hover_shows_the_translated_line() {
    let caller = "<?php\nnamespace App;\nclass Demo {\n\
        \x20   public function go(): void {\n\
        \x20       __('boards.explore');\n\
        \x20       __('boards.nested');\n\
        \x20   }\n}\n";
    let lang = "<?php\nreturn [\n\
        \x20   'explore' => 'Explore :name',\n\
        \x20   'nested' => ['deep' => 'Deep'],\n];\n";
    let (backend, dir) = create_psr4_workspace(
        COMPOSER,
        &[("app/Demo.php", caller), ("lang/en/boards.php", lang)],
    );
    backend.initialized(InitializedParams {}).await;
    let uri = Url::from_file_path(dir.path().join("app/Demo.php")).unwrap();
    open_php(&backend, &uri, caller).await;

    let leaf = hover_text(&backend, &uri, 4, 16).await;
    assert!(
        leaf.contains("`Explore :name`") && leaf.contains("Defined in [`lang/en/boards.php`]"),
        "got {leaf}"
    );

    // A group has no single line, so the hover keeps naming only the file.
    let group = hover_text(&backend, &uri, 5, 16).await;
    assert!(
        group.contains("Defined in [`lang/en/boards.php`]"),
        "got {group}"
    );
}

/// `Env::get()` is the class the `env()` helper calls through, so both
/// spellings name the same variable.
#[tokio::test]
async fn the_env_class_reaches_the_same_declaration_as_the_helper() {
    let caller = "<?php\nnamespace App;\nclass Demo {\n\
        \x20   public function go(): string {\n\
        \x20       return \\Illuminate\\Support\\Env::get('MAIL_MAILER');\n\
        \x20   }\n}\n";
    let (backend, dir) = create_psr4_workspace(
        COMPOSER,
        &[
            ("app/Demo.php", caller),
            (".env", "APP_NAME=Acme\nMAIL_MAILER=log\n"),
        ],
    );

    let character = caller.lines().nth(4).unwrap().find("MAIL_MAILER").unwrap() as u32 + 2;
    let found = definitions(&backend, &dir, "app/Demo.php", 4, character).await;
    assert_eq!(found.len(), 1, "got {found:?}");
    assert!(found[0].uri.as_str().ends_with("/.env"), "got {found:?}");
    assert_eq!(
        found[0].range.start.line, 1,
        "MAIL_MAILER is the second line of .env"
    );
}
