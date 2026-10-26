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
