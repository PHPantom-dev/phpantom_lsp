//! Creating a named Laravel route through the editor's quick-fix edit.

use crate::common::{
    LARAVEL_SRC_COMPOSER, apply_edits, create_initialized_psr4_workspace, create_psr4_workspace,
    edits_for_uri, initialize_with_options, open_document, open_php, position_of, with_code,
    workspace_uri,
};
use phpantom_lsp::Backend;
use tower_lsp_server::LanguageServer;
use tower_lsp_server::gen_lsp_types::*;

const ROUTES: &str = "<?php\nuse Illuminate\\Support\\Facades\\Route;\nRoute::get('/', fn () => 'home')->name('home');\n";
const SOURCE: &str = "<?php\nroute('admin.users.export');\n";

fn diagnostics(backend: &Backend, uri: &Uri, source: &str) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    backend.collect_slow_diagnostics(uri.as_str(), source, &mut diagnostics);
    with_code(diagnostics, "invalid_laravel_route")
}

fn request(uri: &Uri, diagnostics: Vec<Diagnostic>) -> CodeActionParams {
    CodeActionParams {
        text_document: TextDocumentIdentifier { uri: uri.clone() },
        range: Range::new(Position::new(0, 0), Position::new(100, 0)),
        context: CodeActionContext {
            diagnostics,
            only: Some(vec![CodeActionKind::QuickFix]),
            trigger_kind: None,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    }
}

fn route_actions(actions: Vec<CodeActionResponse>) -> Vec<CodeAction> {
    actions
        .into_iter()
        .filter_map(|response| match response {
            CodeActionResponse::CodeAction(action)
                if action.title.starts_with("Create missing route '") =>
            {
                Some(action)
            }
            _ => None,
        })
        .collect()
}

fn actions(
    backend: &Backend,
    uri: &Uri,
    source: &str,
    diagnostics: Vec<Diagnostic>,
) -> Vec<CodeAction> {
    route_actions(backend.handle_code_action(uri.as_str(), source, &request(uri, diagnostics)))
}

#[tokio::test]
async fn create_missing_route_appends_inferred_action_and_resolves_the_diagnostic() {
    let (backend, dir, uri) = create_initialized_psr4_workspace(
        LARAVEL_SRC_COMPOSER,
        &[("src/Links.php", SOURCE), ("routes/web.php", ROUTES)],
        "src/Links.php",
    )
    .await;
    let diagnostics = diagnostics(&backend, &uri, SOURCE);
    assert_eq!(diagnostics.len(), 1);
    let fixes = actions(&backend, &uri, SOURCE, diagnostics.clone());
    assert_eq!(fixes.len(), 1);
    let fix = &fixes[0];
    assert_eq!(fix.title, "Create missing route 'admin.users.export'");
    assert_eq!(fix.kind, Some(CodeActionKind::QuickFix));
    assert_eq!(fix.diagnostics, Some(diagnostics.clone()));
    let target = workspace_uri(&backend, "routes/web.php");
    let edits = edits_for_uri(fix.edit.as_ref().unwrap(), &target);
    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].range.start, edits[0].range.end);
    let updated = apply_edits(ROUTES, &edits);
    assert!(updated.starts_with(ROUTES));
    assert!(updated.contains("\\Illuminate\\Support\\Facades\\Route::get('/admin/users/export', [\\App\\Http\\Controllers\\Admin\\UserController::class, 'export'])->name('admin.users.export');"), "{updated}");
    assert!(fix.command.is_some());
    assert_eq!(
        std::fs::read_to_string(dir.path().join("routes/web.php")).unwrap(),
        ROUTES
    );
    open_php(&backend, &target, &updated).await;
    assert!(self::diagnostics(&backend, &uri, SOURCE).is_empty());
    assert!(
        actions(&backend, &uri, SOURCE, diagnostics).is_empty(),
        "stale diagnostics must not duplicate routes"
    );
}

#[tokio::test]
async fn create_missing_route_preserves_unsaved_target_and_its_line_endings() {
    let (backend, _dir, uri) = create_initialized_psr4_workspace(
        LARAVEL_SRC_COMPOSER,
        &[("src/Links.php", SOURCE), ("routes/web.php", ROUTES)],
        "src/Links.php",
    )
    .await;
    let target = workspace_uri(&backend, "routes/web.php");
    let unsaved = format!("{}// unsaved café 🥐", ROUTES.replace('\n', "\r\n"));
    open_php(&backend, &target, &unsaved).await;
    let fixes = actions(&backend, &uri, SOURCE, diagnostics(&backend, &uri, SOURCE));
    assert_eq!(fixes.len(), 1);
    let updated = apply_edits(
        &unsaved,
        &edits_for_uri(fixes[0].edit.as_ref().unwrap(), &target),
    );
    assert!(updated.starts_with(&unsaved));
    assert!(updated.contains("->name('admin.users.export');"));
    assert!(!updated.replace("\r\n", "").contains('\n'));
}

#[tokio::test]
async fn create_missing_route_creates_conventional_file_without_writing_it() {
    let (backend, dir, uri) = create_initialized_psr4_workspace(
        LARAVEL_SRC_COMPOSER,
        &[("src/Links.php", SOURCE), ("routes/other.php", ROUTES)],
        "src/Links.php",
    )
    .await;
    let fixes = actions(&backend, &uri, SOURCE, diagnostics(&backend, &uri, SOURCE));
    assert_eq!(fixes.len(), 1);
    let edit = fixes[0].edit.as_ref().unwrap();
    let target = workspace_uri(&backend, "routes/web.php");
    let changes = edit.document_changes.as_ref().unwrap();
    assert!(
        matches!(&changes[0], DocumentChange::CreateFile(file) if file.uri == target && file.options.as_ref().unwrap().overwrite == Some(false))
    );
    let created = apply_edits("", &edits_for_uri(edit, &target));
    assert!(created.starts_with("<?php\n"));
    assert!(created.contains("->name('admin.users.export');"));
    assert!(!dir.path().join("routes/web.php").exists());
}

#[tokio::test]
async fn create_missing_route_requires_the_matching_diagnostic_and_request_range() {
    let (backend, _dir, uri) = create_initialized_psr4_workspace(
        LARAVEL_SRC_COMPOSER,
        &[("src/Links.php", SOURCE), ("routes/web.php", ROUTES)],
        "src/Links.php",
    )
    .await;
    assert!(actions(&backend, &uri, SOURCE, vec![]).is_empty());
    let diagnostics = diagnostics(&backend, &uri, SOURCE);
    let mut unrelated = diagnostics.clone();
    unrelated[0].code = Some(Code::String("invalid_laravel_view".into()));
    assert!(actions(&backend, &uri, SOURCE, unrelated).is_empty());
    let mut params = request(&uri, diagnostics);
    params.range = Range::new(Position::new(0, 0), Position::new(0, 3));
    assert!(route_actions(backend.handle_code_action(uri.as_str(), SOURCE, &params)).is_empty());
}

#[tokio::test]
async fn create_missing_route_inserts_php_before_a_closing_tag() {
    let routes = format!("{ROUTES}?>\n<p>preserved HTML</p>\n");
    let (backend, _dir, uri) = create_initialized_psr4_workspace(
        LARAVEL_SRC_COMPOSER,
        &[("src/Links.php", SOURCE), ("routes/web.php", &routes)],
        "src/Links.php",
    )
    .await;
    let fixes = actions(&backend, &uri, SOURCE, diagnostics(&backend, &uri, SOURCE));
    assert_eq!(fixes.len(), 1);
    let target = workspace_uri(&backend, "routes/web.php");
    let updated = apply_edits(
        &routes,
        &edits_for_uri(fixes[0].edit.as_ref().unwrap(), &target),
    );
    assert!(updated.find("->name('admin.users.export');").unwrap() < updated.find("?>").unwrap());
    assert!(updated.ends_with("?>\n<p>preserved HTML</p>\n"));
}

#[tokio::test]
async fn create_missing_route_in_api_file_edits_that_file() {
    let source = format!("{ROUTES}route('users.index');\n");
    let (backend, _dir, uri) = create_initialized_psr4_workspace(
        LARAVEL_SRC_COMPOSER,
        &[("routes/api.php", &source), ("routes/web.php", "<?php\n")],
        "routes/api.php",
    )
    .await;
    let fixes = actions(
        &backend,
        &uri,
        &source,
        diagnostics(&backend, &uri, &source),
    );
    assert_eq!(fixes.len(), 1);
    let updated = apply_edits(
        &source,
        &edits_for_uri(fixes[0].edit.as_ref().unwrap(), &uri),
    );
    assert!(updated.contains("Route::get('/users', [\\App\\Http\\Controllers\\UserController::class, 'index'])->name('users.index');"), "{updated}");
    assert!(
        edits_for_uri(
            fixes[0].edit.as_ref().unwrap(),
            &workspace_uri(&backend, "routes/web.php")
        )
        .is_empty()
    );
}

#[tokio::test]
async fn create_missing_route_from_blade_keeps_diagnostic_coordinates() {
    let template = "<p>🥐</p>\n<a href=\"{{ route('admin.users.export') }}\">Export</a>\n";
    let (backend, _dir) = create_psr4_workspace(
        LARAVEL_SRC_COMPOSER,
        &[
            ("resources/views/links.blade.php", template),
            ("routes/web.php", ROUTES),
        ],
    );
    backend.initialized(InitializedParams {}).await;
    let uri = workspace_uri(&backend, "resources/views/links.blade.php");
    open_document(&backend, &uri, "blade", template).await;
    let virtual_php = backend.blade_virtual_php(uri.as_str()).unwrap();
    let diagnostics = diagnostics(&backend, &uri, &virtual_php);
    assert_eq!(diagnostics.len(), 1);
    let diagnostic = diagnostics.into_iter().next().unwrap();
    assert_eq!(diagnostic.range.start.line, 1);
    let mut params = request(&uri, vec![diagnostic.clone()]);
    params.range = diagnostic.range;
    let fixes = route_actions(
        backend
            .code_action(params)
            .await
            .unwrap()
            .unwrap_or_default(),
    );
    assert_eq!(fixes.len(), 1);
    assert_eq!(fixes[0].diagnostics, Some(vec![diagnostic]));
    let target = workspace_uri(&backend, "routes/web.php");
    assert_eq!(
        edits_for_uri(fixes[0].edit.as_ref().unwrap(), &target).len(),
        1
    );
    assert!(edits_for_uri(fixes[0].edit.as_ref().unwrap(), &uri).is_empty());
}

#[tokio::test]
async fn create_missing_route_ignores_declarations_optional_checks_and_patterns() {
    for source in [
        "<?php Route::get('/new', fn () => 'new')->name('new.route');",
        "<?php Route::has('new.route');",
        "<?php Route::is('new.*');",
        "<?php route($dynamic);",
    ] {
        let (backend, _dir, uri) = create_initialized_psr4_workspace(
            LARAVEL_SRC_COMPOSER,
            &[("src/Links.php", source), ("routes/web.php", ROUTES)],
            "src/Links.php",
        )
        .await;
        let diagnostic = Diagnostic {
            range: Range::new(Position::new(0, 0), Position::new(0, source.len() as u32)),
            code: Some(Code::String("invalid_laravel_route".into())),
            message: "Unknown route".into(),
            ..Default::default()
        };
        assert!(
            actions(&backend, &uri, source, vec![diagnostic]).is_empty(),
            "{source}"
        );
    }
}

#[tokio::test]
async fn create_missing_route_respects_file_creation_capability() {
    for existing in [false, true] {
        let mut files = vec![("src/Links.php", SOURCE), ("routes/other.php", ROUTES)];
        if existing {
            files.push(("routes/web.php", "<?php\n"));
        }
        let (backend, dir) = create_psr4_workspace(LARAVEL_SRC_COMPOSER, &files);
        initialize_with_options(&backend, dir.path(), None).await;
        let uri = workspace_uri(&backend, "src/Links.php");
        open_php(&backend, &uri, SOURCE).await;
        let diagnostics = diagnostics(&backend, &uri, SOURCE);
        assert_eq!(diagnostics.len(), 1);
        let fixes = actions(&backend, &uri, SOURCE, diagnostics);
        assert_eq!(fixes.len(), usize::from(existing));
    }
}

#[tokio::test]
async fn create_missing_route_does_not_offer_a_laravel_edit_in_a_plain_php_project() {
    let (backend, _dir, uri) = create_initialized_psr4_workspace(
        r#"{"autoload":{"psr-4":{"App\\":"src/"}}}"#,
        &[("src/Links.php", SOURCE), ("routes/web.php", ROUTES)],
        "src/Links.php",
    )
    .await;
    let start = position_of(SOURCE, "admin.users.export");
    let diagnostic = Diagnostic {
        range: Range::new(start, Position::new(start.line, start.character + 18)),
        code: Some(Code::String("invalid_laravel_route".into())),
        message: "Unknown route".into(),
        ..Default::default()
    };
    assert!(actions(&backend, &uri, SOURCE, vec![diagnostic]).is_empty());
}

#[tokio::test]
async fn create_missing_route_uses_only_the_enclosing_api_resource_registration() {
    for (registration, expected_file) in [
        (
            "Routes::apiResource('posts', PostController::class)->missing(fn () => redirect()->route('admin.users.export'));",
            "routes/api.php",
        ),
        (
            "Routes::apiResources(['posts' => PostController::class]);\nroute('admin.users.export');",
            "routes/web.php",
        ),
        (
            "Routes::resource('posts', PostController::class)->missing(fn () => redirect()->route('admin.users.export'));",
            "routes/web.php",
        ),
    ] {
        let source =
            format!("<?php\nuse Illuminate\\Support\\Facades\\Route as Routes;\n{registration}\n");
        let (backend, _dir, uri) = create_initialized_psr4_workspace(
            LARAVEL_SRC_COMPOSER,
            &[
                ("src/Links.php", &source),
                ("routes/web.php", ROUTES),
                ("routes/api.php", "<?php\n"),
            ],
            "src/Links.php",
        )
        .await;
        let diagnostics = diagnostics(&backend, &uri, &source);
        assert_eq!(diagnostics.len(), 1, "{registration}");
        let fixes = actions(&backend, &uri, &source, diagnostics);
        assert_eq!(fixes.len(), 1, "{registration}");
        let target = workspace_uri(&backend, expected_file);
        assert_eq!(
            edits_for_uri(fixes[0].edit.as_ref().unwrap(), &target).len(),
            1,
            "{registration}"
        );
    }
}

#[tokio::test]
async fn create_missing_route_does_not_treat_a_declared_route_class_as_the_facade() {
    let source = "<?php\nclass Route { public static function apiResource($value) {} }\nRoute::apiResource(fn () => route('admin.users.export'));\n";
    let (backend, _dir, uri) = create_initialized_psr4_workspace(
        LARAVEL_SRC_COMPOSER,
        &[
            ("src/Links.php", source),
            ("routes/web.php", ROUTES),
            ("routes/api.php", "<?php\n"),
        ],
        "src/Links.php",
    )
    .await;
    let start = position_of(source, "admin.users.export");
    let diagnostic = Diagnostic {
        range: Range::new(start, Position::new(start.line, start.character + 18)),
        code: Some(Code::String("invalid_laravel_route".into())),
        message: "Unknown route".into(),
        ..Default::default()
    };
    let fixes = actions(&backend, &uri, source, vec![diagnostic]);
    assert_eq!(fixes.len(), 1);
    assert_eq!(
        edits_for_uri(
            fixes[0].edit.as_ref().unwrap(),
            &workspace_uri(&backend, "routes/web.php")
        )
        .len(),
        1
    );
}

#[tokio::test]
async fn create_missing_route_rejects_a_target_that_cannot_be_parsed_safely() {
    let (backend, _dir, uri) = create_initialized_psr4_workspace(
        LARAVEL_SRC_COMPOSER,
        &[
            ("src/Links.php", SOURCE),
            ("routes/other.php", ROUTES),
            ("routes/web.php", "<?php Route::get("),
        ],
        "src/Links.php",
    )
    .await;
    let diagnostics = diagnostics(&backend, &uri, SOURCE);
    assert_eq!(diagnostics.len(), 1);
    assert!(actions(&backend, &uri, SOURCE, diagnostics).is_empty());
}

#[tokio::test]
async fn create_missing_route_rejects_stale_source_and_unavailable_workspace_context() {
    let (backend, _dir, uri) = create_initialized_psr4_workspace(
        LARAVEL_SRC_COMPOSER,
        &[("src/Links.php", SOURCE), ("routes/web.php", ROUTES)],
        "src/Links.php",
    )
    .await;
    let diagnostics = diagnostics(&backend, &uri, SOURCE);
    assert_eq!(diagnostics.len(), 1);
    assert!(
        actions(
            &backend,
            &uri,
            "<?php\nroute('admin.users.update');\n",
            diagnostics.clone()
        )
        .is_empty()
    );
    assert!(
        actions(
            &backend,
            &uri,
            "<?php\nroute('home');\n",
            diagnostics.clone()
        )
        .is_empty()
    );
    let unopened = workspace_uri(&backend, "src/Unopened.php");
    assert!(actions(&backend, &unopened, SOURCE, diagnostics.clone()).is_empty());
    let outside = tempfile::tempdir().unwrap();
    let outside_uri = Uri::from_file_path(outside.path().join("Links.php")).unwrap();
    assert!(actions(&backend, &outside_uri, SOURCE, diagnostics.clone()).is_empty());
    *backend.workspace_root().write() = Some(std::path::PathBuf::new());
    assert!(actions(&backend, &uri, SOURCE, diagnostics.clone()).is_empty());
    *backend.workspace_root().write() = None;
    assert!(actions(&backend, &uri, SOURCE, diagnostics).is_empty());
}

#[tokio::test]
async fn create_missing_route_does_not_apply_one_diagnostic_to_another_reference() {
    let source = "<?php\nroute('users.export');\nroute('posts.export');\n";
    let (backend, _dir, uri) = create_initialized_psr4_workspace(
        LARAVEL_SRC_COMPOSER,
        &[("src/Links.php", source), ("routes/web.php", ROUTES)],
        "src/Links.php",
    )
    .await;
    let mut diagnostics = diagnostics(&backend, &uri, source);
    assert_eq!(diagnostics.len(), 2);
    diagnostics.retain(|diagnostic| diagnostic.range.start.line == 1);
    let fixes = actions(&backend, &uri, source, diagnostics);
    assert_eq!(fixes.len(), 1);
    assert_eq!(fixes[0].title, "Create missing route 'users.export'");
}

#[tokio::test]
async fn create_missing_route_rejects_non_utf8_boundaries_from_an_older_index() {
    let (backend, _dir) = create_psr4_workspace(
        LARAVEL_SRC_COMPOSER,
        &[("src/Links.php", SOURCE), ("routes/web.php", ROUTES)],
    );
    backend.initialized(InitializedParams {}).await;
    let uri = workspace_uri(&backend, "src/Links.php");
    backend.update_ast(uri.as_str(), SOURCE);
    let diagnostics = diagnostics(&backend, &uri, SOURCE);
    assert_eq!(diagnostics.len(), 1);
    let mut changed = SOURCE.to_string();
    let start = SOURCE.find("admin.users.export").unwrap();
    changed.replace_range(start - 1..start + 2, "€");
    assert_eq!(changed.len(), SOURCE.len());
    assert!(actions(&backend, &uri, &changed, diagnostics).is_empty());
}
