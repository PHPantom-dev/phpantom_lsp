use crate::common::{goto_definition_at, open_php};
use std::fs;
use std::path::Path;

use phpantom_lsp::Backend;
use tower_lsp_server::LanguageServer;
use tower_lsp_server::gen_lsp_types::*;

const APP: &str = "<?php\nfunction run(): void {\n    new Shared();\n}\n";

fn folder(path: &Path) -> WorkspaceFolder {
    WorkspaceFolder {
        uri: Uri::from_file_path(path).unwrap(),
        name: path.file_name().unwrap().to_string_lossy().into_owned(),
    }
}

/// A workspace of two folders: `app`, which uses a class, and `lib`,
/// which declares it.
fn two_folders() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("app")).unwrap();
    fs::write(dir.path().join("app/run.php"), APP).unwrap();
    fs::create_dir_all(dir.path().join("lib/src")).unwrap();
    fs::write(
        dir.path().join("lib/src/Shared.php"),
        "<?php\nclass Shared {}\n",
    )
    .unwrap();
    dir
}

/// Start a server the way a multi-root client does: rooted at `app`,
/// with every folder of the workspace listed.
async fn start(dir: &Path, root_uri: bool) -> (Backend, InitializeResult) {
    let backend = Backend::new_test();
    let app = dir.join("app");
    #[allow(deprecated)] // `root_uri` is what this generation of clients sends
    let params = InitializeParams {
        root_uri: root_uri.then(|| Uri::from_file_path(&app).unwrap()),
        workspace_folders_initialize_params: WorkspaceFoldersInitializeParams {
            workspace_folders: Some(WorkspaceFolders::WorkspaceFolderList(vec![
                folder(&app),
                folder(&dir.join("lib")),
            ])),
        },
        ..Default::default()
    };
    let result = backend.initialize(params).await.unwrap();
    backend.initialized(InitializedParams {}).await;

    let uri = Uri::from_file_path(app.join("run.php")).unwrap();
    open_php(&backend, &uri, APP).await;
    (backend, result)
}

async fn definition_of_shared(backend: &Backend, dir: &Path) -> Option<DefinitionResponse> {
    let uri = Uri::from_file_path(dir.join("app/run.php")).unwrap();
    goto_definition_at(backend, &uri, 2, 9).await
}

fn points_into_lib(response: Option<DefinitionResponse>, dir: &Path) -> bool {
    let expected = Uri::from_file_path(dir.join("lib/src/Shared.php")).unwrap();
    match response {
        Some(DefinitionResponse::Definition(Definition::Location(location))) => {
            location.uri == expected
        }
        Some(DefinitionResponse::Definition(Definition::LocationList(locations))) => {
            locations.iter().any(|location| location.uri == expected)
        }
        _ => false,
    }
}

/// A class declared in another folder of the workspace resolves from
/// this one, and the server asks to hear when folders come and go.
#[tokio::test]
async fn a_class_in_another_workspace_folder_resolves() {
    let dir = two_folders();
    let (backend, result) = start(dir.path(), true).await;

    assert!(points_into_lib(
        definition_of_shared(&backend, dir.path()).await,
        dir.path()
    ));

    let folders = result
        .capabilities
        .workspace
        .and_then(|workspace| workspace.workspace_folders)
        .expect("workspace folder support must be advertised");
    assert_eq!(folders.supported, Some(true));
    assert_eq!(
        folders.change_notifications,
        Some(ChangeNotifications::Bool(true))
    );
}

/// A client that sends the folder list without the deprecated `rootUri`
/// is rooted at its first folder, which is what the others set `rootUri`
/// to.
#[tokio::test]
async fn the_first_folder_is_the_root_without_a_root_uri() {
    let dir = two_folders();
    let (backend, _) = start(dir.path(), false).await;

    assert_eq!(
        backend.workspace_root().read().as_deref(),
        Some(dir.path().join("app").as_path())
    );
    assert!(points_into_lib(
        definition_of_shared(&backend, dir.path()).await,
        dir.path()
    ));
}

/// Removing the folder stops its classes resolving straight away.
#[tokio::test]
async fn a_removed_folder_stops_resolving() {
    let dir = two_folders();
    let (backend, _) = start(dir.path(), true).await;

    backend
        .did_change_workspace_folders(DidChangeWorkspaceFoldersParams {
            event: WorkspaceFoldersChangeEvent {
                added: Vec::new(),
                removed: vec![folder(&dir.path().join("lib"))],
            },
        })
        .await;

    assert!(!points_into_lib(
        definition_of_shared(&backend, dir.path()).await,
        dir.path()
    ));
}
