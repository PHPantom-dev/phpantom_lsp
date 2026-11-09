use crate::symbol_map::StringKeyKind;

/// A workspace holding `files` on disk, nobody having opened any of them.
fn workspace(files: &[(&str, &str)]) -> (crate::Backend, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    for (path, content) in files {
        let full = dir.path().join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(&full, content).unwrap();
    }
    let backend = crate::Backend::new_test_with_workspace(dir.path().to_path_buf(), vec![]);
    backend.set_is_symfony(false);
    (backend, dir)
}

const THEMES: &[(&str, &str)] = &[
    ("theme/default/index.html", "{% include 'menu.html' %}\n"),
    ("theme/default/menu.html", "<nav></nav>\n"),
    (
        "theme/default/admin/list.html",
        "{% extends 'admin/index.html' %}\n",
    ),
    ("theme/default/admin/index.html", "<main></main>\n"),
    ("docs/page.html", "<p></p>\n"),
];

#[test]
fn templates_nobody_opened_are_found_by_the_workspace_walk() {
    let (backend, dir) = workspace(THEMES);
    backend.ensure_workspace_indexed();

    assert_eq!(
        backend.symfony_declared_names(StringKeyKind::TwigTemplate),
        vec![
            "admin/index.html".to_string(),
            "admin/list.html".to_string(),
            "index.html".to_string(),
            "menu.html".to_string(),
        ]
    );
    let docs = crate::util::path_to_uri(&dir.path().join("docs/page.html"));
    assert!(!backend.is_resource_uri(&docs));
    assert!(backend.symbol_map_for(&docs).is_none());
}

#[test]
fn nothing_is_read_in_a_project_without_twig() {
    let (backend, _dir) = workspace(THEMES);
    backend.set_is_twig(false);
    backend.ensure_workspace_indexed();

    assert!(
        backend
            .symfony_declared_names(StringKeyKind::TwigTemplate)
            .is_empty()
    );
    assert!(backend.twig_roots().is_empty());
}

#[test]
fn a_root_goes_with_the_files_that_made_it() {
    let (backend, dir) = workspace(THEMES);
    backend.ensure_workspace_indexed();

    // The only name resolving under `theme/default/` is gone, along with
    // its templates' names.
    for path in ["theme/default/index.html", "theme/default/admin/list.html"] {
        let uri = crate::util::path_to_uri(&dir.path().join(path));
        std::fs::remove_file(dir.path().join(path)).unwrap();
        backend.clear_file_maps(&uri);
        backend.forget_twig_source(&uri);
    }
    assert!(backend.twig_roots().is_empty());
    assert!(
        backend
            .symfony_declared_names(StringKeyKind::TwigTemplate)
            .is_empty()
    );
}

/// Open `path` in the editor with `content`.
async fn open(
    backend: &crate::Backend,
    dir: &tempfile::TempDir,
    path: &str,
    content: &str,
) -> String {
    let uri = crate::util::path_to_uri(&dir.path().join(path));
    backend
        .on_did_open(tower_lsp_server::gen_lsp_types::DidOpenTextDocumentParams {
            text_document: tower_lsp_server::gen_lsp_types::TextDocumentItem {
                uri: uri.parse().unwrap(),
                language_id: "twig".to_string().into(),
                version: 1,
                text: content.to_string(),
            },
        })
        .await;
    uri
}

#[tokio::test]
async fn an_open_template_is_diagnosed_and_its_namers_follow_its_creation() {
    use std::sync::atomic::Ordering;

    let page = "{% include 'menu.html' %}\n{% include 'footer.html' %}\n";
    let (backend, dir) = workspace(&[
        ("theme/default/menu.html", "<nav></nav>\n"),
        ("theme/default/index.html", page),
    ]);
    backend.ensure_workspace_indexed();
    backend.init_complete.store(true, Ordering::Release);

    let index = open(&backend, &dir, "theme/default/index.html", page).await;
    assert!(backend.diag.pending_uris.lock().contains(&index));
    let mut found = Vec::new();
    backend.collect_template_diagnostics(&index, page, &mut found);
    let messages: Vec<String> = found
        .iter()
        .map(|diagnostic| match &diagnostic.message {
            tower_lsp_server::gen_lsp_types::Message::String(text) => text.clone(),
            other => format!("{other:?}"),
        })
        .collect();
    assert_eq!(messages, vec!["Unknown template 'footer.html'".to_string()]);
    assert_eq!(found[0].range.start.line, 1);

    // Reopening a template the project already declares changes nothing
    // another file is judged by.
    backend.diag.pending_uris.lock().clear();
    open(&backend, &dir, "theme/default/menu.html", "<nav></nav>\n").await;
    assert!(!backend.diag.pending_uris.lock().contains(&index));

    // Creating the missing one does.
    std::fs::write(
        dir.path().join("theme/default/footer.html"),
        "<footer></footer>\n",
    )
    .unwrap();
    open(
        &backend,
        &dir,
        "theme/default/footer.html",
        "<footer></footer>\n",
    )
    .await;
    assert!(backend.diag.pending_uris.lock().contains(&index));
    assert!(
        backend
            .diag
            .full_slow_pass_requested
            .lock()
            .contains(&index)
    );
}

#[test]
fn deleting_a_template_on_disk_rediagnoses_the_files_that_name_it() {
    use std::sync::atomic::Ordering;
    use tower_lsp_server::gen_lsp_types::{DidChangeWatchedFilesParams, FileChangeType, FileEvent};

    let page = "{% include 'menu.html' %}\n";
    let (backend, dir) = workspace(&[
        ("theme/default/menu.html", "<nav></nav>\n"),
        ("theme/default/index.html", page),
        ("theme/default/other.html", "{% include 'index.html' %}\n"),
    ]);
    backend.ensure_workspace_indexed();
    backend.init_complete.store(true, Ordering::Release);
    let index = crate::util::path_to_uri(&dir.path().join("theme/default/index.html"));
    backend
        .open_files
        .write()
        .insert(index.clone(), std::sync::Arc::new(page.to_string()));

    let menu = dir.path().join("theme/default/menu.html");
    std::fs::remove_file(&menu).unwrap();
    backend.apply_watched_file_changes(
        &DidChangeWatchedFilesParams {
            changes: vec![FileEvent {
                uri: crate::util::path_to_uri(&menu).parse().unwrap(),
                kind: FileChangeType::Deleted,
            }],
        },
        dir.path(),
    );

    assert!(backend.diag.pending_uris.lock().contains(&index));
}
