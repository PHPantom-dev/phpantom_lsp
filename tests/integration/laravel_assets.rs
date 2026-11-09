//! Asset paths and Inertia pages: `asset()` against `public/`, `mix()`
//! against the Mix manifest, `@vite` and `Vite::asset()` against the source
//! files, and `Inertia::render()` against the page directories.

use crate::common::{
    LARAVEL_APP_COMPOSER, complete_labels_at, create_psr4_workspace, definitions_at,
    doc_change_edits_for_uri, find_action, get_code_actions_in_range, hover_on, messages_with_code,
    open_blade_template, open_php_file,
};
use tower_lsp_server::LanguageServer;
use tower_lsp_server::gen_lsp_types::*;

/// A controller in `app/` whose one method runs `body`.
fn controller(body: &str) -> String {
    format!(
        "<?php\nnamespace App;\nuse Illuminate\\Support\\Facades\\URL;\nuse Illuminate\\Support\\Facades\\Vite;\nuse Illuminate\\Support\\Facades\\Route;\nuse Inertia\\Inertia;\nclass PageController {{\n    public function show(): mixed {{\n        {body}\n    }}\n}}\n"
    )
}

/// The messages the slow pass reports on `uri` under `code`.
fn diagnostics(
    backend: &phpantom_lsp::Backend,
    uri: &Uri,
    content: &str,
    code: &str,
) -> Vec<String> {
    let mut diags = Vec::new();
    backend.collect_slow_diagnostics(uri.as_str(), content, &mut diags);
    messages_with_code(&diags, code)
}

async fn workspace(
    body: &str,
    files: &[(&str, &str)],
) -> (phpantom_lsp::Backend, tempfile::TempDir, Uri, String) {
    let caller = controller(body);
    let mut all = files.to_vec();
    all.push(("app/PageController.php", &caller));
    let (backend, dir) = create_psr4_workspace(LARAVEL_APP_COMPOSER, &all);
    backend.initialized(InitializedParams {}).await;
    let uri = open_php_file(&backend, "app/PageController.php").await;
    (backend, dir, uri, caller)
}

#[tokio::test]
async fn a_missing_public_asset_is_reported() {
    let (backend, _dir, uri, caller) = workspace(
        "asset('css/app.css');\n        secure_asset('/css/app.css?v=2');\n        URL::asset('css/missing.css');\n        url()->asset('js/missing.js');\n        return asset('storage/avatars/1.png');",
        &[("public/css/app.css", "body {}\n")],
    )
    .await;

    assert_eq!(
        diagnostics(&backend, &uri, &caller, "invalid_laravel_asset"),
        vec![
            "Unknown asset 'css/missing.css'".to_string(),
            "Unknown asset 'js/missing.js'".to_string(),
        ]
    );
    let found = definitions_at(&backend, &uri, &caller, "css/app.css").await;
    assert!(
        found
            .iter()
            .any(|l| l.uri.as_str().ends_with("/public/css/app.css")),
        "go-to-definition should open the asset, got {found:?}"
    );
}

#[tokio::test]
async fn a_mix_entry_is_checked_against_the_manifest() {
    let (backend, _dir, uri, caller) = workspace(
        "mix('js/app.js');\n        return mix('js/admin.js');",
        &[
            (
                "public/mix-manifest.json",
                r#"{"\/js\/app.js": "\/js\/app.js?id=1a2b"}"#,
            ),
            ("public/js/app.js", "\n"),
        ],
    )
    .await;

    assert_eq!(
        diagnostics(&backend, &uri, &caller, "invalid_laravel_mix_asset"),
        vec!["Unknown Mix asset 'js/admin.js'".to_string()]
    );
    let found = definitions_at(&backend, &uri, &caller, "js/app.js").await;
    assert!(
        found
            .iter()
            .any(|l| l.uri.as_str().ends_with("/public/js/app.js")),
        "go-to-definition should open the compiled file, got {found:?}"
    );
}

#[tokio::test]
async fn a_vite_entry_point_is_checked_against_the_source_files() {
    let (backend, dir, uri, caller) = workspace(
        "Vite::asset('resources/images/logo.png');\n        return Vite::content('resources/images/missing.svg');",
        &[
            ("vite.config.js", "export default {};\n"),
            ("resources/images/logo.png", ""),
            ("resources/js/app.js", ""),
            (
                "resources/views/layout.blade.php",
                "<head>\n@vite(['resources/js/app.js', 'resources/css/app.css'])\n</head>\n",
            ),
        ],
    )
    .await;

    assert_eq!(
        diagnostics(&backend, &uri, &caller, "invalid_laravel_vite_asset"),
        vec!["Unknown Vite asset 'resources/images/missing.svg'".to_string()]
    );

    let template = open_blade_template(&backend, "resources/views/layout.blade.php").await;
    let content = std::fs::read_to_string(dir.path().join("resources/views/layout.blade.php"))
        .expect("template");
    assert_eq!(
        diagnostics(&backend, &template, &content, "invalid_laravel_vite_asset"),
        vec!["Unknown Vite asset 'resources/css/app.css'".to_string()]
    );
}

#[tokio::test]
async fn an_inertia_page_is_checked_against_the_page_directories() {
    let (backend, _dir, uri, caller) = workspace(
        "Route::inertia('/about', 'About');\n        inertia('Users/Show');\n        return Inertia::render('Users/Index');",
        &[
            ("resources/js/Pages/About.vue", "<template></template>\n"),
            ("resources/js/Pages/Users/Index.vue", "<template></template>\n"),
        ],
    )
    .await;

    assert_eq!(
        diagnostics(&backend, &uri, &caller, "invalid_inertia_page"),
        vec!["Unknown Inertia page 'Users/Show'".to_string()]
    );
    let found = definitions_at(&backend, &uri, &caller, "Users/Index").await;
    assert!(
        found.iter().any(|l| l
            .uri
            .as_str()
            .ends_with("/resources/js/Pages/Users/Index.vue")),
        "go-to-definition should open the page, got {found:?}"
    );
    let hover = hover_on(&backend, &uri, &caller, "Users/Index").await;
    assert!(
        hover.contains("resources/js/Pages/Users/Index.vue"),
        "hover should name the page file, got {hover}"
    );
}

#[tokio::test]
async fn inertia_pages_follow_config_inertia() {
    let (backend, _dir, uri, caller) = workspace(
        "return Inertia::render('Dashboard');",
        &[
            (
                "config/inertia.php",
                "<?php\nreturn ['pages' => ['paths' => [resource_path('js/Screens')], 'extensions' => ['svelte']]];\n",
            ),
            ("resources/js/Screens/Dashboard.svelte", ""),
            ("resources/js/Pages/Dashboard.vue", ""),
        ],
    )
    .await;

    assert!(
        diagnostics(&backend, &uri, &caller, "invalid_inertia_page").is_empty(),
        "the configured directory holds the page"
    );
    let found = definitions_at(&backend, &uri, &caller, "Dashboard").await;
    assert!(
        found.iter().any(|l| l
            .uri
            .as_str()
            .ends_with("/resources/js/Screens/Dashboard.svelte")),
        "go-to-definition should use the configured directory, got {found:?}"
    );
}

#[tokio::test]
async fn a_missing_inertia_page_can_be_created() {
    let (backend, dir, uri, caller) = workspace(
        "return Inertia::render('Users/Edit');",
        &[(
            "resources/js/Pages/Users/Index.vue",
            "<template></template>\n",
        )],
    )
    .await;

    let line = caller
        .lines()
        .position(|line| line.contains("Users/Edit"))
        .unwrap() as u32;
    let actions = get_code_actions_in_range(
        &backend,
        uri.as_str(),
        &caller,
        Range::new(Position::new(line, 0), Position::new(line, 200)),
    );
    let action =
        find_action(&actions, "Create missing Inertia page").expect("should offer the action");
    let edit = action.edit.as_ref().expect("action should carry an edit");
    let Some(ops) = edit.document_changes.as_ref() else {
        panic!("expected resource operations, got {edit:?}");
    };
    let DocumentChange::CreateFile(create) = &ops[0] else {
        panic!("expected a CreateFile operation, got {ops:?}");
    };
    assert_eq!(
        create.uri.to_file_path().unwrap(),
        dir.path().join("resources/js/Pages/Users/Edit.vue")
    );
    let text: String = doc_change_edits_for_uri(edit, &create.uri)
        .into_iter()
        .map(|edit| edit.new_text)
        .collect();
    assert!(text.contains("<template>"), "{text}");
}

#[tokio::test]
async fn asset_paths_complete_a_directory_at_a_time() {
    let content = "<?php\nasset('css/');\n";
    let (backend, _dir) = create_psr4_workspace(
        LARAVEL_APP_COMPOSER,
        &[
            ("public/css/app.css", ""),
            ("public/css/admin/theme.css", ""),
            ("public/js/app.js", ""),
        ],
    );
    backend.initialized(InitializedParams {}).await;
    let uri = Uri::from_file_path(_dir.path().join("app/usage.php")).unwrap();
    let labels = complete_labels_at(&backend, &uri, content, 1, 11).await;
    assert_eq!(labels, vec!["admin".to_string(), "app.css".to_string()]);
}

#[tokio::test]
async fn inertia_pages_and_mix_entries_complete() {
    let content = "<?php\nInertia\\Inertia::render('Us');\nmix('');\n";
    let (backend, dir) = create_psr4_workspace(
        LARAVEL_APP_COMPOSER,
        &[
            ("resources/js/Pages/Users/Index.vue", ""),
            ("resources/js/Pages/Users/Show.vue", ""),
            ("resources/js/Pages/Dashboard.vue", ""),
            (
                "public/mix-manifest.json",
                r#"{"\/js\/app.js": "\/js\/app.js", "\/css\/app.css": "\/css\/app.css"}"#,
            ),
        ],
    );
    backend.initialized(InitializedParams {}).await;
    let uri = Uri::from_file_path(dir.path().join("app/usage.php")).unwrap();

    let pages = complete_labels_at(&backend, &uri, content, 1, 27).await;
    assert_eq!(
        pages,
        vec!["Users/Index".to_string(), "Users/Show".to_string()]
    );
    let entries = complete_labels_at(&backend, &uri, content, 2, 5).await;
    assert_eq!(
        entries,
        vec!["css/app.css".to_string(), "js/app.js".to_string()]
    );
}
