use crate::common::{
    create_psr4_workspace, create_test_backend_with_full_stubs,
    create_test_backend_with_function_stubs, create_test_backend_with_stubs,
};
use phpantom_lsp::Backend;
use phpantom_lsp::composer::parse_autoload_classmap;
use phpantom_lsp::types::DefineInfo;
use std::collections::HashMap;
use std::fs;
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

// ─── Helper ─────────────────────────────────────────────────────────────────

/// Open a file in the backend and request completion at the given position.
async fn complete_at(
    backend: &Backend,
    uri: &Url,
    text: &str,
    line: u32,
    character: u32,
) -> Vec<CompletionItem> {
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: text.to_string(),
            },
        })
        .await;

    let result = backend
        .completion(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position: Position { line, character },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    match result {
        Some(CompletionResponse::Array(items)) => items,
        Some(CompletionResponse::List(list)) => list.items,
        None => vec![],
    }
}

/// Filter completion items to only those with kind == CLASS.
fn class_items(items: &[CompletionItem]) -> Vec<&CompletionItem> {
    items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::CLASS))
        .collect()
}

/// Extract labels from a list of completion items.
fn labels(items: &[CompletionItem]) -> Vec<&str> {
    items.iter().map(|i| i.label.as_str()).collect()
}

/// Find a completion item by its FQN (stored in the `detail` field).
fn find_by_fqn<'a>(items: &[&'a CompletionItem], fqn: &str) -> Option<&'a CompletionItem> {
    items
        .iter()
        .find(|i| i.detail.as_deref() == Some(fqn))
        .copied()
}

/// Extract FQNs from a list of completion item references (via `detail`).
fn fqns<'a>(items: &'a [&'a CompletionItem]) -> Vec<&'a str> {
    items.iter().filter_map(|i| i.detail.as_deref()).collect()
}

fn function_items(items: &[CompletionItem]) -> Vec<&CompletionItem> {
    items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::FUNCTION))
        .collect()
}

fn constant_items(items: &[CompletionItem]) -> Vec<&CompletionItem> {
    items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::CONSTANT))
        .collect()
}

#[tokio::test]
async fn test_class_name_completion_prioritizes_project_core_explicit_then_transitive() {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    fs::write(
        dir.path().join("composer.json"),
        r#"{
            "name": "demo/project",
            "require": {
                "acme/explicit": "*"
            },
            "autoload": {
                "psr-4": {
                    "App\\": "src/"
                }
            }
        }"#,
    )
    .expect("failed to write composer.json");

    let project_file = dir.path().join("src/RuntimeException.php");
    fs::create_dir_all(project_file.parent().unwrap()).expect("mkdir src");
    fs::write(
        &project_file,
        "<?php\nnamespace App;\nclass RuntimeException extends \\RuntimeException {}\n",
    )
    .expect("write project class");

    let explicit_file = dir
        .path()
        .join("vendor/acme/explicit/src/RuntimeException.php");
    fs::create_dir_all(explicit_file.parent().unwrap()).expect("mkdir explicit");
    fs::write(
        &explicit_file,
        "<?php\nnamespace Acme\\Explicit;\nclass RuntimeException extends \\RuntimeException {}\n",
    )
    .expect("write explicit class");

    let transitive_file = dir
        .path()
        .join("vendor/transitive/dep/src/RuntimeException.php");
    fs::create_dir_all(transitive_file.parent().unwrap()).expect("mkdir transitive");
    fs::write(
        &transitive_file,
        "<?php\nnamespace Transitive\\Dep;\nclass RuntimeException extends \\RuntimeException {}\n",
    )
    .expect("write transitive class");

    let composer_dir = dir.path().join("vendor/composer");
    fs::create_dir_all(&composer_dir).expect("mkdir vendor/composer");
    fs::write(
        composer_dir.join("installed.json"),
        r#"{
            "packages": [
                {
                    "name": "acme/explicit",
                    "install-path": "../acme/explicit",
                    "autoload": { "psr-4": { "Acme\\Explicit\\": ["src/"] } }
                },
                {
                    "name": "transitive/dep",
                    "install-path": "../transitive/dep",
                    "autoload": { "psr-4": { "Transitive\\Dep\\": ["src/"] } }
                }
            ]
        }"#,
    )
    .expect("write installed.json");

    let backend = create_test_backend_with_full_stubs();
    *backend.workspace_root().write() = Some(dir.path().to_path_buf());
    backend.initialized(InitializedParams {}).await;

    // Use a file in the App namespace so that App\RuntimeException is
    // same-namespace (source '1') and all other RuntimeException variants
    // are non-imported (source '2').  This isolates origin_tier ordering
    // for non-imported classes.  (Without a namespace the global core
    // RuntimeException would be same-namespace and sort first.)
    let uri = Url::parse("file:///app.php").unwrap();
    let text = "<?php\nnamespace App;\nnew RuntimeE\n";
    let items = complete_at(&backend, &uri, text, 2, 12).await;
    let classes = class_items(&items);

    let project_pos = classes
        .iter()
        .position(|i| i.detail.as_deref() == Some("App\\RuntimeException"))
        .expect("project RuntimeException missing");
    let core_pos = classes
        .iter()
        .position(|i| i.detail.as_deref() == Some("RuntimeException"))
        .expect("core RuntimeException missing");
    let explicit_pos = classes
        .iter()
        .position(|i| i.detail.as_deref() == Some("Acme\\Explicit\\RuntimeException"))
        .expect("explicit vendor RuntimeException missing");
    let transitive_pos = classes
        .iter()
        .position(|i| i.detail.as_deref() == Some("Transitive\\Dep\\RuntimeException"))
        .expect("transitive RuntimeException missing");

    assert!(
        project_pos < core_pos && core_pos < explicit_pos && explicit_pos < transitive_pos,
        "expected project < core < explicit vendor < transitive vendor, got order: {:?}",
        fqns(&classes)
    );
}

/// After a `composer` change the origin ranking must be rebuilt: a package
/// that was transitive but is now an explicit dependency should be promoted
/// ahead of the packages that remain transitive.
#[tokio::test]
async fn test_composer_change_refreshes_completion_provenance() {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    let composer_path = dir.path().join("composer.json");
    // Initially neither vendor package is required, so both are transitive.
    fs::write(
        &composer_path,
        r#"{ "name": "demo/project", "require": {} }"#,
    )
    .expect("write composer.json");

    // Equal-length class names so the only ranking difference is the origin
    // tier: "ExAlpha" (stays transitive) sorts before "ExOmega" by name,
    // and promoting ExOmega's package must flip that order.
    for (pkg, ns, class) in [
        ("stays/trans", "Stays\\Trans", "ExAlpha"),
        ("promoted/dep", "Promoted\\Dep", "ExOmega"),
    ] {
        let file = dir.path().join(format!("vendor/{pkg}/src/{class}.php"));
        fs::create_dir_all(file.parent().unwrap()).expect("mkdir pkg");
        fs::write(
            &file,
            format!("<?php\nnamespace {ns};\nclass {class} {{}}\n"),
        )
        .expect("write vendor class");
    }

    let composer_dir = dir.path().join("vendor/composer");
    fs::create_dir_all(&composer_dir).expect("mkdir vendor/composer");
    fs::write(
        composer_dir.join("installed.json"),
        r#"{
            "packages": [
                { "name": "stays/trans", "install-path": "../stays/trans",
                  "autoload": { "psr-4": { "Stays\\Trans\\": ["src/"] } } },
                { "name": "promoted/dep", "install-path": "../promoted/dep",
                  "autoload": { "psr-4": { "Promoted\\Dep\\": ["src/"] } } }
            ]
        }"#,
    )
    .expect("write installed.json");

    let backend = create_test_backend_with_full_stubs();
    *backend.workspace_root().write() = Some(dir.path().to_path_buf());
    backend.initialized(InitializedParams {}).await;

    let uri = Url::parse("file:///app.php").unwrap();
    let text = "<?php\nnew Ex\n";
    let dep_pos = |fqns: &[&str], fqn: &str| fqns.iter().position(|f| *f == fqn);

    // Before the change both are transitive (tier 3), so they tie-break by
    // name: ExAlpha before ExOmega.
    let items = complete_at(&backend, &uri, text, 1, 6).await;
    let classes = class_items(&items);
    let before = fqns(&classes);
    assert!(
        dep_pos(&before, "Stays\\Trans\\ExAlpha") < dep_pos(&before, "Promoted\\Dep\\ExOmega"),
        "expected transitive packages tie-broken by name, got: {before:?}"
    );

    // A `composer require promoted/dep` promotes it to an explicit
    // dependency; the editor notifies us of the composer.json change.
    fs::write(
        &composer_path,
        r#"{ "name": "demo/project", "require": { "promoted/dep": "*" } }"#,
    )
    .expect("rewrite composer.json");
    backend
        .did_change_watched_files(DidChangeWatchedFilesParams {
            changes: vec![FileEvent {
                uri: Url::from_file_path(&composer_path).unwrap(),
                typ: FileChangeType::CHANGED,
            }],
        })
        .await;

    // Now promoted/dep is explicit (tier 2) and must sort ahead of the
    // still-transitive stays/trans (tier 3), flipping the order.
    let items = complete_at(&backend, &uri, text, 1, 6).await;
    let classes = class_items(&items);
    let after = fqns(&classes);
    assert!(
        dep_pos(&after, "Promoted\\Dep\\ExOmega") < dep_pos(&after, "Stays\\Trans\\ExAlpha"),
        "expected newly-explicit dependency promoted ahead of transitive one, got: {after:?}"
    );
}

#[tokio::test]
async fn test_function_completion_prioritizes_project_core_explicit_then_transitive() {
    let backend = create_test_backend_with_full_stubs();
    let helper_uri = Url::parse("file:///workspace/src/helpers.php").unwrap();
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: helper_uri,
                language_id: "php".to_string(),
                version: 1,
                text: "<?php\nnamespace App;\nfunction runtime_helper() {}\n".to_string(),
            },
        })
        .await;

    backend
        .stub_function_index_mut()
        .insert("runtime_helper", "<?php function runtime_helper() {} ");

    backend.autoload_function_index().write().insert(
        "Acme\\Explicit\\runtime_helper",
        std::path::PathBuf::from("/vendor/acme/explicit.php"),
    );
    backend.autoload_function_origin_index().write().insert(
        "Acme\\Explicit\\runtime_helper",
        phpantom_lsp::ClassCompletionOrigin::VendorExplicit,
    );
    backend.autoload_function_index().write().insert(
        "Transitive\\Dep\\runtime_helper",
        std::path::PathBuf::from("/vendor/transitive/dep.php"),
    );
    backend.autoload_function_origin_index().write().insert(
        "Transitive\\Dep\\runtime_helper",
        phpantom_lsp::ClassCompletionOrigin::VendorTransitive,
    );

    let uri = Url::parse("file:///app.php").unwrap();
    let text = "<?php\nruntime_h\n";
    let items = complete_at(&backend, &uri, text, 1, 9).await;
    let funcs = function_items(&items);
    let sorts: Vec<String> = funcs.iter().filter_map(|i| i.sort_text.clone()).collect();
    // sort_text format: {quality}{source}{origin}_name
    // source_tier sorts before origin_tier.
    assert!(
        sorts.iter().any(|s| s.starts_with("b00_"))
            && sorts.iter().any(|s| s.starts_with("b01_"))
            && sorts.iter().any(|s| s.starts_with("b12_"))
            && sorts.iter().any(|s| s.starts_with("b13_")),
        "expected project/core/explicit/transitive sort tiers in function completions, got: {sorts:?}"
    );
}

#[tokio::test]
async fn test_constant_completion_prioritizes_project_core_explicit_then_transitive() {
    let backend = create_test_backend_with_full_stubs();

    backend.global_defines().write().insert(
        "APP_RUNTIME_FLAG".to_string(),
        DefineInfo {
            file_uri: "file:///workspace/src/constants.php".to_string(),
            name_offset: 0,
            value: Some("'project'".to_string()),
        },
    );
    backend.stub_constant_index_mut().insert(
        "APP_RUNTIME_FLAG_CORE",
        "<?php const APP_RUNTIME_FLAG_CORE = 1;",
    );

    backend.autoload_constant_index().write().insert(
        "APP_RUNTIME_FLAG_VENDOR".to_string(),
        std::path::PathBuf::from("/vendor/acme/flag.php"),
    );
    backend.autoload_constant_origin_index().write().insert(
        "APP_RUNTIME_FLAG_VENDOR".to_string(),
        phpantom_lsp::ClassCompletionOrigin::VendorExplicit,
    );
    backend.autoload_constant_index().write().insert(
        "APP_RUNTIME_FLAG_TRANSITIVE".to_string(),
        std::path::PathBuf::from("/vendor/transitive/flag.php"),
    );
    backend.autoload_constant_origin_index().write().insert(
        "APP_RUNTIME_FLAG_TRANSITIVE".to_string(),
        phpantom_lsp::ClassCompletionOrigin::VendorTransitive,
    );

    let uri = Url::parse("file:///app.php").unwrap();
    let text = "<?php\nAPP_RUNTIME_FLAG\n";
    let items = complete_at(&backend, &uri, text, 1, 16).await;
    let consts = constant_items(&items);
    let names: Vec<&str> = consts.iter().map(|i| i.label.as_str()).collect();
    assert!(
        names.iter().position(|n| *n == "APP_RUNTIME_FLAG").unwrap()
            < names
                .iter()
                .position(|n| *n == "APP_RUNTIME_FLAG_TRANSITIVE")
                .unwrap(),
        "expected project constant before transitive constant, names={names:?}"
    );
}

// ─── extract_partial_class_name tests ───────────────────────────────────────

#[test]
fn test_extract_partial_class_name_simple() {
    let content = "<?php\nnew Dat\n";
    let result = Backend::extract_partial_class_name(
        content,
        Position {
            line: 1,
            character: 7,
        },
    );
    assert_eq!(result, Some("Dat".to_string()));
}

#[test]
fn test_extract_partial_class_name_with_namespace() {
    let content = "<?php\nnew App\\Models\\Us\n";
    let result = Backend::extract_partial_class_name(
        content,
        Position {
            line: 1,
            character: 19,
        },
    );
    assert_eq!(result, Some("App\\Models\\Us".to_string()));
}

#[test]
fn test_extract_partial_class_name_variable_returns_none() {
    let content = "<?php\n$var\n";
    let result = Backend::extract_partial_class_name(
        content,
        Position {
            line: 1,
            character: 4,
        },
    );
    assert!(
        result.is_none(),
        "Variables ($var) should not trigger class name completion"
    );
}

#[test]
fn test_extract_partial_class_name_empty_returns_none() {
    let content = "<?php\n\n";
    let result = Backend::extract_partial_class_name(
        content,
        Position {
            line: 1,
            character: 0,
        },
    );
    assert!(
        result.is_none(),
        "Empty position should not trigger class name completion"
    );
}

#[test]
fn test_extract_partial_class_name_after_arrow_returns_none() {
    let content = "<?php\n$this->meth\n";
    let result = Backend::extract_partial_class_name(
        content,
        Position {
            line: 1,
            character: 11,
        },
    );
    assert!(
        result.is_none(),
        "After -> should not trigger class name completion"
    );
}

#[test]
fn test_extract_partial_class_name_after_double_colon_returns_none() {
    let content = "<?php\nFoo::bar\n";
    let result = Backend::extract_partial_class_name(
        content,
        Position {
            line: 1,
            character: 8,
        },
    );
    assert!(
        result.is_none(),
        "After :: should not trigger class name completion"
    );
}

#[test]
fn test_extract_partial_class_name_type_hint_context() {
    let content = "<?php\nfunction foo(Str $x) {}\n";
    // Cursor after "Str" at position 16
    let result = Backend::extract_partial_class_name(
        content,
        Position {
            line: 1,
            character: 16,
        },
    );
    assert_eq!(result, Some("Str".to_string()));
}

#[test]
fn test_extract_partial_class_name_with_leading_backslash() {
    let content = "<?php\nnew \\Run\n";
    let result = Backend::extract_partial_class_name(
        content,
        Position {
            line: 1,
            character: 8,
        },
    );
    assert_eq!(
        result,
        Some("\\Run".to_string()),
        "Leading backslash should be included in the partial"
    );
}

// ─── Backslash-prefixed completion matching ─────────────────────────────────

/// When the user types `\Unit`, the leading `\` should be stripped for
/// matching so that stub class `UnitEnum` is still found.
#[tokio::test]
async fn test_class_name_completion_with_leading_backslash() {
    let backend = create_test_backend_with_stubs();
    let uri = Url::parse("file:///backslash.php").unwrap();
    // Use a type-hint context (Any) so the interface stub isn't filtered.
    let text = concat!("<?php\n", "function foo(\\Unit $x) {}\n",);

    let items = complete_at(&backend, &uri, text, 1, 17).await;
    let classes = class_items(&items);
    let class_labels: Vec<&str> = classes.iter().map(|i| i.label.as_str()).collect();

    assert!(
        class_labels.contains(&"UnitEnum"),
        "Typing '\\Unit' should match 'UnitEnum', got: {:?}",
        class_labels
    );
}

/// When the user types `\Backed`, the leading `\` should be stripped for
/// matching so that stub class `BackedEnum` is still found.
#[tokio::test]
async fn test_class_name_completion_backslash_backed() {
    let backend = create_test_backend_with_stubs();
    let uri = Url::parse("file:///backslash2.php").unwrap();
    // Use a type-hint context (Any) so the interface stub isn't filtered.
    let text = concat!("<?php\n", "function foo(\\Backed $x) {}\n",);

    let items = complete_at(&backend, &uri, text, 1, 19).await;
    let classes = class_items(&items);
    let class_labels: Vec<&str> = classes.iter().map(|i| i.label.as_str()).collect();

    assert!(
        class_labels.contains(&"BackedEnum"),
        "Typing '\\Backed' should match 'BackedEnum', got: {:?}",
        class_labels
    );
}

/// FQN prefix like `\App\Models\Us` should still match via the
/// namespace portion — the leading `\` must not break matching.
#[tokio::test]
async fn test_class_name_completion_fqn_prefix() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": {
                    "App\\": "src/"
                }
            }
        }"#,
        &[(
            "src/Models/User.php",
            concat!(
                "<?php\n",
                "namespace App\\Models;\n",
                "class User {\n",
                "    public function getName(): string { return ''; }\n",
                "}\n",
            ),
        )],
    );

    let uri = Url::parse("file:///fqn_test.php").unwrap();
    let text = concat!("<?php\n", "new \\Us\n",);

    // Open the User file so it's in ast_map
    let user_uri = Url::parse(&format!(
        "file://{}",
        _dir.path().join("src/Models/User.php").display()
    ))
    .unwrap();
    let user_content = std::fs::read_to_string(_dir.path().join("src/Models/User.php")).unwrap();
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: user_uri,
                language_id: "php".to_string(),
                version: 1,
                text: user_content,
            },
        })
        .await;

    let items = complete_at(&backend, &uri, text, 1, 7).await;
    let classes = class_items(&items);

    // With a leading `\`, FQN mode is active: the label for namespaced
    // classes is the full FQN, not the short name.
    let user_item = classes
        .iter()
        .find(|i| i.detail.as_deref() == Some("App\\Models\\User"));
    assert!(
        user_item.is_some(),
        "Typing '\\Us' should match App\\Models\\User, got details: {:?}",
        classes
            .iter()
            .map(|i| i.detail.as_deref())
            .collect::<Vec<_>>()
    );
}

// ─── Stub class name completion tests ───────────────────────────────────────

#[tokio::test]
async fn test_class_name_completion_includes_stubs() {
    let backend = create_test_backend_with_stubs();

    let uri = Url::parse("file:///test.php").unwrap();

    // Use instanceof context so interface stubs pass through.
    // Check UnitEnum is found when typing "Unit"
    let text_unit = concat!("<?php\n", "$x instanceof Unit\n",);
    let items_unit = complete_at(&backend, &uri, text_unit, 1, 18).await;
    let classes_unit = class_items(&items_unit);
    let labels_unit: Vec<&str> = classes_unit.iter().map(|i| i.label.as_str()).collect();

    assert!(
        !classes_unit.is_empty(),
        "Should return class name completions when typing a class name"
    );
    assert!(
        labels_unit.contains(&"UnitEnum"),
        "Should include stub interface 'UnitEnum', got: {:?}",
        labels_unit
    );

    // Check BackedEnum is found when typing "Backed"
    let text_backed = concat!("<?php\n", "$x instanceof Backed\n",);
    let items_backed = complete_at(&backend, &uri, text_backed, 1, 20).await;
    let classes_backed = class_items(&items_backed);
    let labels_backed: Vec<&str> = classes_backed.iter().map(|i| i.label.as_str()).collect();

    assert!(
        labels_backed.contains(&"BackedEnum"),
        "Should include stub interface 'BackedEnum', got: {:?}",
        labels_backed
    );
}

#[tokio::test]
async fn test_class_name_completion_not_triggered_for_variables() {
    let backend = create_test_backend_with_stubs();

    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!("<?php\n", "$unit\n",);

    let items = complete_at(&backend, &uri, text, 1, 5).await;
    let classes = class_items(&items);

    // Should NOT return class completions when typing a variable
    assert!(
        classes.is_empty(),
        "Should not return class name completions after $, got: {:?}",
        labels(&items)
    );
}

// ─── Use-imported class completion tests ────────────────────────────────────

#[tokio::test]
async fn test_class_name_completion_includes_use_imports() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": {
                    "Acme\\": "src/"
                }
            }
        }"#,
        &[(
            "src/Service.php",
            concat!(
                "<?php\n",
                "namespace Acme;\n",
                "class Service {\n",
                "    public function run(): void {}\n",
                "}\n",
            ),
        )],
    );

    // Open the Service file so it populates fqn_uri_index
    let service_path = _dir.path().join("src/Service.php");
    let service_uri = Url::parse(&format!("file://{}", service_path.display())).unwrap();
    let service_content = std::fs::read_to_string(&service_path).unwrap();
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: service_uri,
                language_id: "php".to_string(),
                version: 1,
                text: service_content,
            },
        })
        .await;

    let uri = Url::parse("file:///app.php").unwrap();
    let text = concat!("<?php\n", "use Acme\\Service;\n", "new Ser\n",);

    let items = complete_at(&backend, &uri, text, 2, 7).await;
    let classes = class_items(&items);
    let class_fqns = fqns(&classes);

    assert!(
        class_fqns.contains(&"Acme\\Service"),
        "Should include use-imported class 'Acme\\Service', got: {:?}",
        class_fqns
    );

    // Check that the detail shows the FQN and label is the short name
    let service_item = find_by_fqn(&classes, "Acme\\Service").unwrap();
    assert_eq!(
        service_item.label, "Service",
        "Label should be the short name"
    );
    assert_eq!(
        service_item.detail.as_deref(),
        Some("Acme\\Service"),
        "Detail should show FQN"
    );
}

#[tokio::test]
async fn test_class_name_completion_use_import_has_higher_sort_priority() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": {
                    "Acme\\": "src/"
                }
            }
        }"#,
        &[(
            "src/Widget.php",
            concat!("<?php\n", "namespace Acme;\n", "class Widget {}\n",),
        )],
    );

    // Open the Widget file so it populates fqn_uri_index
    let widget_path = _dir.path().join("src/Widget.php");
    let widget_uri = Url::parse(&format!("file://{}", widget_path.display())).unwrap();
    let widget_content = std::fs::read_to_string(&widget_path).unwrap();
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: widget_uri,
                language_id: "php".to_string(),
                version: 1,
                text: widget_content,
            },
        })
        .await;

    let uri = Url::parse("file:///app.php").unwrap();
    let text = concat!("<?php\n", "use Acme\\Widget;\n", "new Wid\n",);

    let items = complete_at(&backend, &uri, text, 2, 7).await;
    let classes = class_items(&items);

    let widget_item = find_by_fqn(&classes, "Acme\\Widget").unwrap();
    let sort = widget_item.sort_text.as_deref().unwrap_or("");
    // New format: {quality}{origin}{tier}{affinity:4}{demote}_{name}
    // Tier '0' = use-imported, at position 2.
    assert!(
        sort.len() > 2 && &sort[2..3] == "0",
        "Use-imported classes should have source tier '0' at position 2, got: {:?}",
        sort
    );
}

// ─── Same-namespace class completion tests ──────────────────────────────────

#[tokio::test]
async fn test_class_name_completion_same_namespace() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": {
                    "App\\": "src/"
                }
            }
        }"#,
        &[
            (
                "src/UserService.php",
                concat!(
                    "<?php\n",
                    "namespace App;\n",
                    "class UserService {\n",
                    "    public function find(): void {}\n",
                    "}\n",
                ),
            ),
            (
                "src/Controller.php",
                concat!(
                    "<?php\n",
                    "namespace App;\n",
                    "class Controller {\n",
                    "    public function index() {\n",
                    "        new User\n",
                    "    }\n",
                    "}\n",
                ),
            ),
        ],
    );

    // Open the UserService file first so it gets into the ast_map
    let service_uri = Url::parse("file:///service.php").unwrap();
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: service_uri.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: concat!(
                    "<?php\n",
                    "namespace App;\n",
                    "class UserService {\n",
                    "    public function find(): void {}\n",
                    "}\n",
                )
                .to_string(),
            },
        })
        .await;

    // Open the Controller file — same namespace "App"
    let uri = Url::parse("file:///controller.php").unwrap();
    let text = concat!(
        "<?php\n",
        "namespace App;\n",
        "class Controller {\n",
        "    public function index() {\n",
        "        new User\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 4, 16).await;
    let classes = class_items(&items);
    let class_fqns = fqns(&classes);

    assert!(
        class_fqns.contains(&"App\\UserService"),
        "Should include same-namespace class 'App\\UserService', got: {:?}",
        class_fqns
    );

    // Same-namespace should have source tier '1' at position 1
    // (source_tier sorts before origin_tier).
    let service_item = find_by_fqn(&classes, "App\\UserService").unwrap();
    let sort = service_item.sort_text.as_deref().unwrap_or("");
    assert!(
        sort.len() > 1 && &sort[1..2] == "1",
        "Same-namespace classes should have source tier '1' at position 1, got: {:?}",
        sort
    );
}

// ─── Classmap-based class name completion tests ─────────────────────────────

#[tokio::test]
async fn test_class_name_completion_from_classmap() {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    fs::write(
        dir.path().join("composer.json"),
        r#"{"name": "test/project"}"#,
    )
    .expect("failed to write composer.json");

    // Create the classmap
    let composer_dir = dir.path().join("vendor").join("composer");
    fs::create_dir_all(&composer_dir).expect("failed to create vendor/composer");
    fs::write(
        composer_dir.join("autoload_classmap.php"),
        concat!(
            "<?php\n",
            "$vendorDir = dirname(__DIR__);\n",
            "$baseDir = dirname($vendorDir);\n",
            "\n",
            "return array(\n",
            "    'Illuminate\\\\Support\\\\Collection' => $vendorDir . '/laravel/framework/src/Illuminate/Support/Collection.php',\n",
            "    'Illuminate\\\\Database\\\\Eloquent\\\\Model' => $vendorDir . '/laravel/framework/src/Illuminate/Database/Eloquent/Model.php',\n",
            "    'Carbon\\\\Carbon' => $vendorDir . '/nesbot/carbon/src/Carbon/Carbon.php',\n",
            ");\n",
        ),
    )
    .expect("failed to write autoload_classmap.php");

    let backend = Backend::new_test_with_workspace(dir.path().to_path_buf(), vec![]);

    // Populate class index from classmap
    let classmap = parse_autoload_classmap(dir.path(), "vendor");
    assert_eq!(classmap.len(), 3);
    {
        let mut idx = backend.fqn_uri_index().write();
        for (fqn, path) in &classmap {
            idx.insert(fqn.clone(), Url::from_file_path(path).unwrap().to_string());
        }
    }

    let uri = Url::parse("file:///app.php").unwrap();

    // Check Collection matches prefix "Coll"
    let text = concat!("<?php\n", "new Coll\n",);
    let items = complete_at(&backend, &uri, text, 1, 8).await;
    let classes = class_items(&items);
    let class_fqns = fqns(&classes);

    assert!(
        class_fqns.contains(&"Illuminate\\Support\\Collection"),
        "Should include classmap class 'Illuminate\\Support\\Collection', got: {:?}",
        class_fqns
    );

    // Check Model matches prefix "Mo"
    let text_mo = concat!("<?php\n", "new Mo\n",);
    let items_mo = complete_at(&backend, &uri, text_mo, 1, 6).await;
    let classes_mo = class_items(&items_mo);
    let fqns_mo = fqns(&classes_mo);
    assert!(
        fqns_mo.contains(&"Illuminate\\Database\\Eloquent\\Model"),
        "Should include classmap class 'Illuminate\\Database\\Eloquent\\Model', got: {:?}",
        fqns_mo
    );

    // Check Carbon matches prefix "Car"
    let text_car = concat!("<?php\n", "new Car\n",);
    let items_car = complete_at(&backend, &uri, text_car, 1, 7).await;
    let classes_car = class_items(&items_car);
    let fqns_car = fqns(&classes_car);
    assert!(
        fqns_car.contains(&"Carbon\\Carbon"),
        "Should include classmap class 'Carbon\\Carbon', got: {:?}",
        fqns_car
    );

    // Check that detail shows the FQN
    let collection = find_by_fqn(&classes, "Illuminate\\Support\\Collection")
        .expect("Should have a Collection item with FQN Illuminate\\Support\\Collection in detail");
    assert_eq!(
        collection.detail.as_deref(),
        Some("Illuminate\\Support\\Collection"),
        "Detail should show FQN for classmap entries"
    );
}

// ─── class_index-based class name completion tests ──────────────────────────

#[tokio::test]
async fn test_class_name_completion_from_class_index() {
    let backend = create_test_backend_with_stubs();

    // Manually populate the class_index with a discovered class
    {
        let mut idx = backend.fqn_uri_index().write();
        idx.insert(
            "App\\Models\\User".to_string(),
            "file:///app/Models/User.php".to_string(),
        );
        idx.insert(
            "App\\Models\\Order".to_string(),
            "file:///app/Models/Order.php".to_string(),
        );
    }

    let uri = Url::parse("file:///test.php").unwrap();

    // Check User matches prefix "Us"
    let text = concat!("<?php\n", "new Us\n",);
    let items = complete_at(&backend, &uri, text, 1, 6).await;
    let classes = class_items(&items);
    let class_fqns = fqns(&classes);

    assert!(
        class_fqns.contains(&"App\\Models\\User"),
        "Should include class_index class 'App\\Models\\User', got: {:?}",
        class_fqns
    );

    // Check Order matches prefix "Or"
    let text_or = concat!("<?php\n", "new Or\n",);
    let items_or = complete_at(&backend, &uri, text_or, 1, 6).await;
    let classes_or = class_items(&items_or);
    let fqns_or = fqns(&classes_or);

    assert!(
        fqns_or.contains(&"App\\Models\\Order"),
        "Should include class_index class 'App\\Models\\Order', got: {:?}",
        fqns_or
    );
}

// ─── Deduplication tests ────────────────────────────────────────────────────

#[tokio::test]
async fn test_class_name_completion_deduplicates_by_fqn() {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    fs::write(
        dir.path().join("composer.json"),
        r#"{"name": "test/project"}"#,
    )
    .expect("failed to write composer.json");

    let composer_dir = dir.path().join("vendor").join("composer");
    fs::create_dir_all(&composer_dir).expect("failed to create vendor/composer");
    fs::write(
        composer_dir.join("autoload_classmap.php"),
        concat!(
            "<?php\n",
            "$vendorDir = dirname(__DIR__);\n",
            "$baseDir = dirname($vendorDir);\n",
            "\n",
            "return array(\n",
            "    'Acme\\\\Duplicated' => $vendorDir . '/acme/src/Duplicated.php',\n",
            ");\n",
        ),
    )
    .expect("failed to write autoload_classmap.php");

    let backend = Backend::new_test_with_workspace(dir.path().to_path_buf(), vec![]);

    // Add to class index
    let classmap = parse_autoload_classmap(dir.path(), "vendor");
    {
        let mut idx = backend.fqn_uri_index().write();
        for (fqn, path) in &classmap {
            idx.insert(fqn.clone(), Url::from_file_path(path).unwrap().to_string());
        }
    }

    // Also add to class_index (same FQN)
    {
        let mut idx = backend.fqn_uri_index().write();
        idx.insert(
            "Acme\\Duplicated".to_string(),
            "file:///acme/src/Duplicated.php".to_string(),
        );
    }

    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!("<?php\n", "new Dup\n",);

    let items = complete_at(&backend, &uri, text, 1, 7).await;
    let classes = class_items(&items);

    // Count how many times "Acme\Duplicated" appears (by FQN in detail)
    let dup_count = classes
        .iter()
        .filter(|i| i.detail.as_deref() == Some("Acme\\Duplicated"))
        .count();
    assert_eq!(
        dup_count, 1,
        "Should deduplicate classes with the same FQN, got {} occurrences",
        dup_count
    );
}

// ─── Context sensitivity tests ──────────────────────────────────────────────

#[tokio::test]
async fn test_class_name_completion_after_new_keyword() {
    let backend = create_test_backend_with_stubs();

    let uri = Url::parse("file:///test.php").unwrap();
    // Use a locally-defined class since `new` context correctly
    // filters out interfaces (BackedEnum, UnitEnum).
    let text = concat!(
        "<?php\n",
        "class BackupService {\n",
        "    function bar() {\n",
        "        $x = new Back\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 3, 21).await;
    let classes = class_items(&items);
    let class_labels: Vec<&str> = classes.iter().map(|i| i.label.as_str()).collect();

    assert!(
        class_labels.contains(&"BackupService"),
        "Should offer class names after 'new' keyword, got: {:?}",
        class_labels
    );
    // BackedEnum is an interface — it should be filtered out in new context.
    assert!(
        !class_labels.contains(&"BackedEnum"),
        "Should not offer interface stubs after 'new', got: {:?}",
        class_labels
    );
}

#[tokio::test]
async fn test_class_name_completion_in_type_hint() {
    let backend = create_test_backend_with_stubs();

    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!("<?php\n", "function process(Unit $x) {}\n",);

    // Cursor after "Unit" at character 21
    let items = complete_at(&backend, &uri, text, 1, 21).await;
    let classes = class_items(&items);
    let class_labels: Vec<&str> = classes.iter().map(|i| i.label.as_str()).collect();

    assert!(
        class_labels.contains(&"UnitEnum"),
        "Should offer class names in type hint position, got: {:?}",
        class_labels
    );
}

#[tokio::test]
async fn test_class_name_completion_in_extends_clause() {
    let backend = create_test_backend_with_stubs();

    let uri = Url::parse("file:///test.php").unwrap();
    // BackedEnum is an interface, so it belongs in `interface extends`,
    // not `class extends`.
    let text = concat!("<?php\n", "interface MyEnum extends Back\n",);

    let items = complete_at(&backend, &uri, text, 1, 32).await;
    let classes = class_items(&items);
    let class_labels: Vec<&str> = classes.iter().map(|i| i.label.as_str()).collect();

    assert!(
        class_labels.contains(&"BackedEnum"),
        "Should offer interface names in interface extends clause, got: {:?}",
        class_labels
    );
}

// ─── No class completion when member access is detected ─────────────────────

#[tokio::test]
async fn test_class_name_completion_not_after_arrow() {
    let backend = create_test_backend_with_stubs();

    // Open a file where `->` triggers member completion
    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Foo {\n",
        "    public function bar(): void {}\n",
        "}\n",
        "$f = new Foo();\n",
        "$f->ba\n",
    );

    let items = complete_at(&backend, &uri, text, 5, 6).await;

    // Should get member completion items, NOT class name items
    let classes = class_items(&items);
    assert!(
        classes.is_empty(),
        "Should NOT return class name completions after ->, got: {:?}",
        labels(&items)
    );
}

// ─── All items have CLASS kind ──────────────────────────────────────────────

#[tokio::test]
async fn test_class_name_completion_items_have_class_kind() {
    let backend = create_test_backend_with_stubs();

    let uri = Url::parse("file:///test.php").unwrap();
    // Use instanceof context so the interface stub passes the filter.
    let text = concat!("<?php\n", "$x instanceof Uni\n",);

    let items = complete_at(&backend, &uri, text, 1, 17).await;
    let classes = class_items(&items);

    assert!(
        !classes.is_empty(),
        "Should have at least one class completion"
    );

    for item in &classes {
        assert_eq!(
            item.kind,
            Some(CompletionItemKind::CLASS),
            "All class name completions should have kind=CLASS, item '{}' has kind={:?}",
            item.label,
            item.kind
        );
    }
}

// ─── Combined sources test ──────────────────────────────────────────────────

#[tokio::test]
async fn test_class_name_completion_combines_all_sources() {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    fs::write(
        dir.path().join("composer.json"),
        r#"{"name": "test/project"}"#,
    )
    .expect("failed to write composer.json");

    let composer_dir = dir.path().join("vendor").join("composer");
    fs::create_dir_all(&composer_dir).expect("failed to create vendor/composer");
    fs::write(
        composer_dir.join("autoload_classmap.php"),
        concat!(
            "<?php\n",
            "$vendorDir = dirname(__DIR__);\n",
            "$baseDir = dirname($vendorDir);\n",
            "return array(\n",
            "    'Vendor\\\\ClassmapClass' => $vendorDir . '/vendor/src/ClassmapClass.php',\n",
            ");\n",
        ),
    )
    .expect("failed to write autoload_classmap.php");

    let mut stubs: HashMap<&str, &str> = HashMap::new();
    stubs.insert(
        "StubClass",
        "<?php\nclass StubClass {\n    public function stubMethod(): void {}\n}\n",
    );
    let backend = Backend::new_test_with_stubs(stubs);
    *backend.workspace_root().write() = Some(dir.path().to_path_buf());

    // Populate class index from classmap
    let classmap = parse_autoload_classmap(dir.path(), "vendor");
    {
        let mut idx = backend.fqn_uri_index().write();
        for (fqn, path) in &classmap {
            idx.insert(fqn.clone(), Url::from_file_path(path).unwrap().to_string());
        }
    }

    // Add a class_index entry
    {
        let mut idx = backend.fqn_uri_index().write();
        idx.insert(
            "App\\IndexedClass".to_string(),
            "file:///app/IndexedClass.php".to_string(),
        );
    }

    // Open a file with a use statement — use a prefix that matches
    // classes from all three sources.  All test class names end with
    // "Class", so the prefix "Cl" only matches "ClassmapClass".
    // Instead we use separate checks per source.
    let uri = Url::parse("file:///test.php").unwrap();

    // Check stubs: "Stub" matches "StubClass"
    let text_stub = concat!("<?php\n", "use App\\IndexedClass;\n", "new Stub\n",);
    let items_stub = complete_at(&backend, &uri, text_stub, 2, 8).await;
    let classes_stub = class_items(&items_stub);
    let labels_stub: Vec<&str> = classes_stub.iter().map(|i| i.label.as_str()).collect();
    assert!(
        labels_stub.contains(&"StubClass"),
        "Should include stub class, got: {:?}",
        labels_stub
    );

    // Check classmap: "Classmap" matches "ClassmapClass"
    let text_cm = concat!("<?php\n", "use App\\IndexedClass;\n", "new Classmap\n",);
    let items_cm = complete_at(&backend, &uri, text_cm, 2, 12).await;
    let classes_cm = class_items(&items_cm);
    let fqns_cm = fqns(&classes_cm);
    assert!(
        fqns_cm.contains(&"Vendor\\ClassmapClass"),
        "Should include classmap class, got: {:?}",
        fqns_cm
    );

    // Check use-import / class_index: "Indexed" matches "IndexedClass"
    let text_idx = concat!("<?php\n", "use App\\IndexedClass;\n", "new Indexed\n",);
    let items_idx = complete_at(&backend, &uri, text_idx, 2, 11).await;
    let classes_idx = class_items(&items_idx);
    let fqns_idx = fqns(&classes_idx);
    assert!(
        fqns_idx.contains(&"App\\IndexedClass"),
        "Should include use-imported / class_index class, got: {:?}",
        fqns_idx
    );
}

// ─── Insert text tests ─────────────────────────────────────────────────────

#[tokio::test]
async fn test_class_name_completion_insert_text_is_short_name() {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    fs::write(
        dir.path().join("composer.json"),
        r#"{"name": "test/project"}"#,
    )
    .expect("failed to write composer.json");

    let composer_dir = dir.path().join("vendor").join("composer");
    fs::create_dir_all(&composer_dir).expect("failed to create vendor/composer");
    fs::write(
        composer_dir.join("autoload_classmap.php"),
        concat!(
            "<?php\n",
            "$vendorDir = dirname(__DIR__);\n",
            "$baseDir = dirname($vendorDir);\n",
            "return array(\n",
            "    'Deep\\\\Nested\\\\Namespace\\\\MyClass' => $vendorDir . '/pkg/src/MyClass.php',\n",
            ");\n",
        ),
    )
    .expect("failed to write autoload_classmap.php");

    let backend = Backend::new_test_with_workspace(dir.path().to_path_buf(), vec![]);
    let classmap = parse_autoload_classmap(dir.path(), "vendor");
    {
        let mut idx = backend.fqn_uri_index().write();
        for (fqn, path) in &classmap {
            idx.insert(fqn.clone(), Url::from_file_path(path).unwrap().to_string());
        }
    }

    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!("<?php\n", "new My\n",);

    let items = complete_at(&backend, &uri, text, 1, 6).await;
    let classes = class_items(&items);

    let my_class = find_by_fqn(&classes, "Deep\\Nested\\Namespace\\MyClass")
        .expect("Should find Deep\\Nested\\Namespace\\MyClass by FQN");
    assert_eq!(
        my_class.insert_text.as_deref(),
        Some("MyClass()$0"),
        "insert_text should be the short class name with parens in `new` context"
    );
    assert_eq!(
        my_class.insert_text_format,
        Some(InsertTextFormat::SNIPPET),
        "insert_text_format should be Snippet in `new` context"
    );
    assert_eq!(
        my_class.detail.as_deref(),
        Some("Deep\\Nested\\Namespace\\MyClass"),
        "detail should show the FQN"
    );
}

// ─── Auto-import (additional_text_edits) tests ──────────────────────────────

/// Selecting a classmap class should add `use FQN;` after existing use statements.
#[tokio::test]
async fn test_auto_import_classmap_class_adds_use_statement() {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    fs::write(
        dir.path().join("composer.json"),
        r#"{"name": "test/project"}"#,
    )
    .expect("failed to write composer.json");

    let composer_dir = dir.path().join("vendor").join("composer");
    fs::create_dir_all(&composer_dir).expect("failed to create vendor/composer");
    fs::write(
        composer_dir.join("autoload_classmap.php"),
        concat!(
            "<?php\n",
            "$vendorDir = dirname(__DIR__);\n",
            "return array(\n",
            "    'Illuminate\\\\Support\\\\Collection' => $vendorDir . '/laravel/framework/src/Collection.php',\n",
            ");\n",
        ),
    )
    .expect("failed to write autoload_classmap.php");

    let backend = Backend::new_test_with_workspace(dir.path().to_path_buf(), vec![]);
    let classmap = parse_autoload_classmap(dir.path(), "vendor");
    {
        let mut idx = backend.fqn_uri_index().write();
        for (fqn, path) in &classmap {
            idx.insert(fqn.clone(), Url::from_file_path(path).unwrap().to_string());
        }
    }

    let uri = Url::parse("file:///app.php").unwrap();
    let text = concat!(
        "<?php\n",
        "namespace App;\n",
        "use App\\Helpers\\Foo;\n",
        "\n",
        "new Coll\n",
    );

    let items = complete_at(&backend, &uri, text, 4, 8).await;
    let collection = items
        .iter()
        .find(|i| i.detail.as_deref() == Some("Illuminate\\Support\\Collection"))
        .expect("Should have Collection completion");

    let edits = collection
        .additional_text_edits
        .as_ref()
        .expect("Classmap class should have additional_text_edits for auto-import");

    assert_eq!(edits.len(), 1);
    assert_eq!(
        edits[0].new_text, "use Illuminate\\Support\\Collection;\n",
        "Should insert a use statement for the FQN"
    );
    // Should insert after the last `use` line (line 2), so at line 3
    assert_eq!(
        edits[0].range.start,
        Position {
            line: 3,
            character: 0,
        },
        "Should insert after the last existing use statement"
    );
}

/// Selecting a class_index class should add `use FQN;` too.
#[tokio::test]
async fn test_auto_import_class_index_adds_use_statement() {
    let backend = create_test_backend_with_stubs();

    {
        let mut idx = backend.fqn_uri_index().write();
        idx.insert(
            "App\\Services\\PaymentService".to_string(),
            "file:///app/Services/PaymentService.php".to_string(),
        );
    }

    let uri = Url::parse("file:///controller.php").unwrap();
    let text = concat!(
        "<?php\n",
        "namespace App\\Controllers;\n",
        "\n",
        "new Payment\n",
    );

    let items = complete_at(&backend, &uri, text, 3, 11).await;
    let payment = items
        .iter()
        .find(|i| i.detail.as_deref() == Some("App\\Services\\PaymentService"))
        .expect("Should have PaymentService completion");

    let edits = payment
        .additional_text_edits
        .as_ref()
        .expect("class_index class should have additional_text_edits");

    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].new_text, "\nuse App\\Services\\PaymentService;\n",);
    // No existing use statements; insert after namespace (line 1) at line 2
    assert_eq!(
        edits[0].range.start,
        Position {
            line: 2,
            character: 0,
        },
    );
}

/// Non-namespaced classes (e.g. `DateTime`) should NOT get auto-import edits.
#[tokio::test]
async fn test_no_auto_import_for_non_namespaced_class() {
    let mut stubs: HashMap<&str, &str> = HashMap::new();
    stubs.insert(
        "DateTime",
        "<?php\nclass DateTime {\n    public function format(string $f): string {}\n}\n",
    );
    let backend = Backend::new_test_with_stubs(stubs);

    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!("<?php\n", "new DateT\n",);

    let items = complete_at(&backend, &uri, text, 1, 9).await;
    let dt = items
        .iter()
        .find(|i| i.label == "DateTime")
        .expect("Should have DateTime completion");

    assert!(
        dt.additional_text_edits.is_none(),
        "Non-namespaced class should not get auto-import edits, got: {:?}",
        dt.additional_text_edits
    );
}

/// Already-imported classes (source 1) should NOT get auto-import edits.
#[tokio::test]
async fn test_no_auto_import_for_already_imported_class() {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    fs::write(
        dir.path().join("composer.json"),
        r#"{"name": "test/project"}"#,
    )
    .expect("failed to write composer.json");

    let composer_dir = dir.path().join("vendor").join("composer");
    fs::create_dir_all(&composer_dir).expect("failed to create vendor/composer");
    fs::write(
        composer_dir.join("autoload_classmap.php"),
        concat!(
            "<?php\n",
            "$vendorDir = dirname(__DIR__);\n",
            "return array(\n",
            "    'Illuminate\\\\Support\\\\Collection' => $vendorDir . '/laravel/framework/src/Collection.php',\n",
            ");\n",
        ),
    )
    .expect("failed to write autoload_classmap.php");

    let backend = Backend::new_test_with_workspace(dir.path().to_path_buf(), vec![]);
    let classmap = parse_autoload_classmap(dir.path(), "vendor");
    {
        let mut idx = backend.fqn_uri_index().write();
        for (fqn, path) in &classmap {
            idx.insert(fqn.clone(), Url::from_file_path(path).unwrap().to_string());
        }
    }

    let uri = Url::parse("file:///app.php").unwrap();
    let text = concat!(
        "<?php\n",
        "use Illuminate\\Support\\Collection;\n",
        "\n",
        "new Coll\n",
    );

    let items = complete_at(&backend, &uri, text, 3, 8).await;
    // The use-imported version (source 1) should be the first match
    let collection = items
        .iter()
        .find(|i| i.detail.as_deref() == Some("Illuminate\\Support\\Collection"))
        .expect("Should have Collection completion");

    assert!(
        collection.additional_text_edits.is_none(),
        "Already-imported class should not get auto-import edits"
    );
}

/// When there are no use statements and no namespace, insert after `<?php`.
#[tokio::test]
async fn test_auto_import_inserts_after_php_open_tag() {
    let backend = create_test_backend_with_stubs();

    {
        let mut idx = backend.fqn_uri_index().write();
        idx.insert(
            "Vendor\\Lib\\Widget".to_string(),
            "file:///vendor/lib/Widget.php".to_string(),
        );
    }

    let uri = Url::parse("file:///bare.php").unwrap();
    let text = concat!("<?php\n", "\n", "new Wid\n",);

    let items = complete_at(&backend, &uri, text, 2, 7).await;
    let widget = items
        .iter()
        .find(|i| i.detail.as_deref() == Some("Vendor\\Lib\\Widget"))
        .expect("Should have Widget completion");

    let edits = widget
        .additional_text_edits
        .as_ref()
        .expect("Should have auto-import edit");

    assert_eq!(edits[0].new_text, "use Vendor\\Lib\\Widget;\n");
    // Insert after `<?php` (line 0), so at line 1
    assert_eq!(
        edits[0].range.start,
        Position {
            line: 1,
            character: 0,
        },
    );
}

/// Trait `use` statements inside a class body must NOT be mistaken for
/// namespace `use` imports.  The auto-import should insert after the
/// top-level `use` statements, not after `use HasSlug;` etc.
#[tokio::test]
async fn test_auto_import_not_confused_by_trait_use_in_class_body() {
    let backend = create_test_backend_with_stubs();

    {
        let mut idx = backend.fqn_uri_index().write();
        idx.insert(
            "Cassandra\\DefaultCluster".to_string(),
            "file:///vendor/cassandra/DefaultCluster.php".to_string(),
        );
    }

    let uri = Url::parse("file:///showcase.php").unwrap();
    let text = concat!(
        "<?php\n",                                          // line 0
        "\n",                                               // line 1
        "namespace Demo;\n",                                // line 2
        "\n",                                               // line 3
        "use Exception;\n",                                 // line 4
        "use Stringable;\n",                                // line 5
        "\n",                                               // line 6
        "class User extends Model implements Renderable\n", // line 7
        "{\n",                                              // line 8
        "    use HasTimestamps;\n",                         // line 9
        "    use HasSlug;\n",                               // line 10
        "\n",                                               // line 11
        "    function test() {\n",                          // line 12
        "        new Default\n",                            // line 13
        "    }\n",                                          // line 14
        "}\n",                                              // line 15
    );

    let items = complete_at(&backend, &uri, text, 13, 19).await;
    let cluster = items
        .iter()
        .find(|i| i.detail.as_deref() == Some("Cassandra\\DefaultCluster"))
        .expect("Should have DefaultCluster completion");

    let edits = cluster
        .additional_text_edits
        .as_ref()
        .expect("Should have auto-import edit");

    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].new_text, "use Cassandra\\DefaultCluster;\n",);
    // `Cassandra\DefaultCluster` sorts before `Exception` alphabetically,
    // so it should be inserted before `use Exception;` (line 4),
    // NOT after `use HasSlug;` (line 10) which is a trait import inside the class.
    assert_eq!(
        edits[0].range.start,
        Position {
            line: 4,
            character: 0,
        },
        "Auto-import should be inserted alphabetically among top-level use statements"
    );
}

/// Global classes (no namespace separator in FQN, e.g. `PDO`) should get a
/// `use PDO;` import when the current file declares a namespace.
#[tokio::test]
async fn test_auto_import_global_class_when_file_has_namespace() {
    let mut stubs: HashMap<&str, &str> = HashMap::new();
    stubs.insert(
        "PDO",
        "<?php\nclass PDO {\n    public function query(string $q): mixed {}\n}\n",
    );
    let backend = Backend::new_test_with_stubs(stubs);

    let uri = Url::parse("file:///app.php").unwrap();
    let text = concat!(
        "<?php\n",              // line 0
        "\n",                   // line 1
        "namespace App\\Db;\n", // line 2
        "\n",                   // line 3
        "new PD\n",             // line 4
    );

    let items = complete_at(&backend, &uri, text, 4, 6).await;
    let pdo = items
        .iter()
        .find(|i| i.label == "PDO")
        .expect("Should have PDO completion");

    let edits = pdo
        .additional_text_edits
        .as_ref()
        .expect("Global class should get auto-import when file has a namespace");

    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].new_text, "\nuse PDO;\n");
    // Insert after `namespace App\Db;` (line 2), so at line 3
    assert_eq!(
        edits[0].range.start,
        Position {
            line: 3,
            character: 0,
        },
    );
}

/// Global classes should NOT get an auto-import when the file has no namespace.
/// (This complements `test_no_auto_import_for_non_namespaced_class`.)
#[tokio::test]
async fn test_no_auto_import_global_class_when_file_has_no_namespace() {
    let mut stubs: HashMap<&str, &str> = HashMap::new();
    stubs.insert(
        "PDO",
        "<?php\nclass PDO {\n    public function query(string $q): mixed {}\n}\n",
    );
    let backend = Backend::new_test_with_stubs(stubs);

    let uri = Url::parse("file:///test.php").unwrap();
    let text = concat!("<?php\n", "new PD\n",);

    let items = complete_at(&backend, &uri, text, 1, 6).await;
    let pdo = items
        .iter()
        .find(|i| i.label == "PDO")
        .expect("Should have PDO completion");

    assert!(
        pdo.additional_text_edits.is_none(),
        "Global class should NOT get auto-import when file has no namespace, got: {:?}",
        pdo.additional_text_edits
    );
}

/// When a file has a namespace and existing use statements, the global class
/// import should be inserted after the last use statement.
#[tokio::test]
async fn test_auto_import_global_class_inserts_after_existing_use_statements() {
    let mut stubs: HashMap<&str, &str> = HashMap::new();
    stubs.insert(
        "PDO",
        "<?php\nclass PDO {\n    public function query(string $q): mixed {}\n}\n",
    );
    let backend = Backend::new_test_with_stubs(stubs);

    let uri = Url::parse("file:///app.php").unwrap();
    let text = concat!(
        "<?php\n",                                // line 0
        "\n",                                     // line 1
        "namespace App\\Service;\n",              // line 2
        "\n",                                     // line 3
        "use App\\Repository\\UserRepository;\n", // line 4
        "use App\\Entity\\User;\n",               // line 5
        "\n",                                     // line 6
        "new PD\n",                               // line 7
    );

    let items = complete_at(&backend, &uri, text, 7, 6).await;
    let pdo = items
        .iter()
        .find(|i| i.label == "PDO")
        .expect("Should have PDO completion");

    let edits = pdo
        .additional_text_edits
        .as_ref()
        .expect("Global class should get auto-import when file has a namespace");

    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].new_text, "use PDO;\n");
    // Insert after last use statement (line 5), so at line 6
    assert_eq!(
        edits[0].range.start,
        Position {
            line: 6,
            character: 0,
        },
    );
}

// ─── `new` context filtering tests ─────────────────────────────────────────

/// After `new`, completion should NOT include constants or functions.
#[tokio::test]
async fn test_new_context_excludes_constants_and_functions() {
    let backend = create_test_backend_with_function_stubs();
    let uri = Url::parse("file:///test_new_no_const_func.php").unwrap();
    let text = concat!("<?php\n", "new Date\n",);

    let items = complete_at(&backend, &uri, text, 1, 8).await;

    // Should find DateTime (a class stub).
    let has_class = items
        .iter()
        .any(|i| i.kind == Some(CompletionItemKind::CLASS));
    assert!(has_class, "Should include class completions after `new`");

    // Should NOT find any constants.
    let constants: Vec<&str> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::CONSTANT))
        .map(|i| i.label.as_str())
        .collect();
    assert!(
        constants.is_empty(),
        "Should not include constants after `new`, got: {:?}",
        constants
    );

    // Should NOT find any functions.
    let functions: Vec<&str> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::FUNCTION))
        .map(|i| i.label.as_str())
        .collect();
    assert!(
        functions.is_empty(),
        "Should not include functions after `new`, got: {:?}",
        functions
    );
}

/// Without `new`, completion SHOULD include constants and functions.
#[tokio::test]
async fn test_non_new_context_includes_constants_and_functions() {
    let backend = create_test_backend_with_function_stubs();
    let uri = Url::parse("file:///test_no_new.php").unwrap();
    // Bare identifier context — no `new` keyword.
    let text = concat!("<?php\n", "PHP_\n",);

    let items = complete_at(&backend, &uri, text, 1, 4).await;

    let constants: Vec<&str> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::CONSTANT))
        .map(|i| i.label.as_str())
        .collect();
    assert!(
        !constants.is_empty(),
        "Should include constants without `new`, got: {:?}",
        labels(&items)
    );
}

/// After `new`, loaded abstract classes should be excluded.
#[tokio::test]
async fn test_new_context_excludes_loaded_abstract_class() {
    let backend = create_test_backend_with_stubs();
    let uri = Url::parse("file:///test_new_abstract.php").unwrap();
    let text = concat!(
        "<?php\n",
        "namespace App;\n",
        "abstract class AbstractWidget {}\n",
        "class ConcreteWidget extends AbstractWidget {}\n",
        "new Wid\n",
    );

    let items = complete_at(&backend, &uri, text, 4, 7).await;
    let classes = class_items(&items);
    let class_fqns = fqns(&classes);

    assert!(
        class_fqns.contains(&"App\\ConcreteWidget"),
        "Should include concrete class, got: {:?}",
        class_fqns
    );
    assert!(
        !class_fqns.contains(&"App\\AbstractWidget"),
        "Should exclude loaded abstract class, got: {:?}",
        class_fqns
    );
}

/// After `new`, loaded interfaces should be excluded.
#[tokio::test]
async fn test_new_context_excludes_loaded_interface() {
    let backend = create_test_backend_with_stubs();
    let uri = Url::parse("file:///test_new_iface.php").unwrap();
    let text = concat!(
        "<?php\n",
        "namespace App;\n",
        "interface Renderable {}\n",
        "class HtmlRenderer implements Renderable {}\n",
        "new Render\n",
    );

    let items = complete_at(&backend, &uri, text, 4, 10).await;
    let classes = class_items(&items);
    let class_fqns = fqns(&classes);

    assert!(
        class_fqns.contains(&"App\\HtmlRenderer"),
        "Should include concrete class, got: {:?}",
        class_fqns
    );
    assert!(
        !class_fqns.contains(&"App\\Renderable"),
        "Should exclude loaded interface, got: {:?}",
        class_fqns
    );
}

/// After `new`, loaded traits should be excluded.
#[tokio::test]
async fn test_new_context_excludes_loaded_trait() {
    let backend = create_test_backend_with_stubs();
    let uri = Url::parse("file:///test_new_trait.php").unwrap();
    let text = concat!(
        "<?php\n",
        "namespace App;\n",
        "trait Loggable {}\n",
        "class Logger { use Loggable; }\n",
        "new Logg\n",
    );

    let items = complete_at(&backend, &uri, text, 4, 8).await;
    let classes = class_items(&items);
    let class_fqns = fqns(&classes);

    assert!(
        class_fqns.contains(&"App\\Logger"),
        "Should include concrete class, got: {:?}",
        class_fqns
    );
    assert!(
        !class_fqns.contains(&"App\\Loggable"),
        "Should exclude loaded trait, got: {:?}",
        class_fqns
    );
}

/// After `new`, loaded enums should be excluded (enums cannot be instantiated).
#[tokio::test]
async fn test_new_context_excludes_loaded_enum() {
    let backend = create_test_backend_with_stubs();
    let uri = Url::parse("file:///test_new_enum.php").unwrap();
    let text = concat!(
        "<?php\n",
        "namespace App;\n",
        "enum ColorEnum { case Red; case Blue; }\n",
        "class ColorPicker {}\n",
        "new Color\n",
    );

    let items = complete_at(&backend, &uri, text, 4, 9).await;
    let classes = class_items(&items);
    let class_fqns = fqns(&classes);

    assert!(
        class_fqns.contains(&"App\\ColorPicker"),
        "Should include concrete class, got: {:?}",
        class_fqns
    );
    assert!(
        !class_fqns.contains(&"App\\ColorEnum"),
        "Should exclude loaded enum, got: {:?}",
        class_fqns
    );
}

/// After `new`, unloaded classmap entries whose name matches non-instantiable
/// naming conventions should sort below normal names:
/// - ends/starts with "Abstract"
/// - ends with "Interface"
/// - starts with `I[A-Z]` (C#-style interface prefix)
/// - starts/ends with case-sensitive "Base"
#[tokio::test]
async fn test_new_context_demotes_likely_non_instantiable_classmap() {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    fs::write(
        dir.path().join("composer.json"),
        r#"{"name": "test/project"}"#,
    )
    .expect("failed to write composer.json");

    let composer_dir = dir.path().join("vendor").join("composer");
    fs::create_dir_all(&composer_dir).expect("failed to create vendor/composer");
    fs::write(
        composer_dir.join("autoload_classmap.php"),
        concat!(
            "<?php\n",
            "$vendorDir = dirname(__DIR__);\n",
            "$baseDir = dirname($vendorDir);\n",
            "return array(\n",
            // Concrete — should NOT be demoted
            "    'Vendor\\\\ConcreteHandler' => $vendorDir . '/src/ConcreteHandler.php',\n",
            "    'Vendor\\\\ImageHandler' => $vendorDir . '/src/ImageHandler.php',\n",
            "    'Vendor\\\\DatabaseHandler' => $vendorDir . '/src/DatabaseHandler.php',\n",
            "    'Vendor\\\\BaselineHandler' => $vendorDir . '/src/BaselineHandler.php',\n",
            // Abstract prefix/suffix — should be demoted
            "    'Vendor\\\\AbstractHandler' => $vendorDir . '/src/AbstractHandler.php',\n",
            "    'Vendor\\\\HandlerAbstract' => $vendorDir . '/src/HandlerAbstract.php',\n",
            // Interface suffix — should be demoted
            "    'Vendor\\\\HandlerInterface' => $vendorDir . '/src/HandlerInterface.php',\n",
            // I[A-Z] prefix — should be demoted
            "    'Vendor\\\\IHandler' => $vendorDir . '/src/IHandler.php',\n",
            // Base[A-Z] prefix — should be demoted
            "    'Vendor\\\\BaseHandler' => $vendorDir . '/src/BaseHandler.php',\n",
            ");\n",
        ),
    )
    .expect("failed to write autoload_classmap.php");

    let backend = Backend::new_test();
    *backend.workspace_root().write() = Some(dir.path().to_path_buf());

    let classmap = parse_autoload_classmap(dir.path(), "vendor");
    {
        let mut idx = backend.fqn_uri_index().write();
        for (fqn, path) in &classmap {
            idx.insert(fqn.clone(), Url::from_file_path(path).unwrap().to_string());
        }
    }

    let uri = Url::parse("file:///test_new_demote.php").unwrap();
    let text = concat!("<?php\n", "new Handler\n",);

    let items = complete_at(&backend, &uri, text, 1, 11).await;
    let classes = class_items(&items);

    // New sort_text format: {quality}{origin}{tier}{affinity:4}{demote}{gap:3}_{name}
    // Demote flag is at position 7 ('0' = normal, '1' = demoted).
    // Within the same match quality group, demoted items sort after
    // normal items.

    // Helper: extract the demote flag (position 7) from a sort_text.
    let demote_flag = |item: &CompletionItem| -> char {
        item.sort_text
            .as_deref()
            .and_then(|s| s.chars().nth(7))
            .unwrap_or('?')
    };

    let concrete = find_by_fqn(&classes, "Vendor\\ConcreteHandler")
        .expect("Should find Vendor\\ConcreteHandler");

    // ConcreteHandler should NOT be demoted.
    assert_eq!(
        demote_flag(concrete),
        '0',
        "ConcreteHandler should not be demoted, sort_text: {:?}",
        concrete.sort_text
    );

    // These should all be demoted (demote flag = '1').
    let demoted_names = [
        "Vendor\\AbstractHandler",
        "Vendor\\HandlerAbstract",
        "Vendor\\HandlerInterface",
        "Vendor\\IHandler",
        "Vendor\\BaseHandler",
    ];
    for name in &demoted_names {
        let item = find_by_fqn(&classes, name)
            .unwrap_or_else(|| panic!("Should find {} (unloaded, included but demoted)", name));
        assert_eq!(
            demote_flag(item),
            '1',
            "{} should be demoted (flag '1'), sort_text: {:?}",
            name,
            item.sort_text
        );
    }

    // Within the same match quality tier, demoted items sort after
    // non-demoted items.  Compare pairs that share a match quality.
    // ConcreteHandler and AbstractHandler both start with a different
    // letter than the prefix "Handler", so both are substring matches
    // (quality 'c').  The demote flag should push Abstract below Concrete.
    let abstract_h = find_by_fqn(&classes, "Vendor\\AbstractHandler")
        .expect("Should find Vendor\\AbstractHandler");
    assert!(
        concrete.sort_text < abstract_h.sort_text,
        "ConcreteHandler ({:?}) should sort before AbstractHandler ({:?}) \
         (same match quality, demoted vs normal)",
        concrete.sort_text,
        abstract_h.sort_text
    );

    // ImageHandler starts with "I" but second char is lowercase — NOT demoted.
    let image =
        find_by_fqn(&classes, "Vendor\\ImageHandler").expect("Should find Vendor\\ImageHandler");
    assert_eq!(
        demote_flag(image),
        '0',
        "ImageHandler should not be demoted, sort_text: {:?}",
        image.sort_text
    );

    // DatabaseHandler contains "base" but not case-sensitive "Base" — NOT demoted.
    let database = find_by_fqn(&classes, "Vendor\\DatabaseHandler")
        .expect("Should find Vendor\\DatabaseHandler");
    assert_eq!(
        demote_flag(database),
        '0',
        "DatabaseHandler should not be demoted, sort_text: {:?}",
        database.sort_text
    );

    // BaselineHandler starts with "Base" but 5th char is lowercase — NOT demoted.
    let baseline = find_by_fqn(&classes, "Vendor\\BaselineHandler")
        .expect("Should find Vendor\\BaselineHandler");
    assert_eq!(
        demote_flag(baseline),
        '0',
        "BaselineHandler should not be demoted, sort_text: {:?}",
        baseline.sort_text
    );
}

/// After `new`, unloaded stub entries whose name starts with "Abstract"
/// should sort below normal stub names.
#[tokio::test]
async fn test_new_context_excludes_abstract_stubs() {
    let mut stubs: HashMap<&str, &str> = HashMap::new();
    stubs.insert("ConcreteService", "<?php\nclass ConcreteService {}\n");
    stubs.insert(
        "AbstractService",
        "<?php\nabstract class AbstractService {}\n",
    );
    let backend = Backend::new_test_with_stubs(stubs);

    let uri = Url::parse("file:///test_new_demote_stubs.php").unwrap();
    let text = concat!("<?php\n", "new Service\n",);

    let items = complete_at(&backend, &uri, text, 1, 11).await;
    let classes = class_items(&items);
    let class_labels: Vec<&str> = classes.iter().map(|i| i.label.as_str()).collect();

    assert!(
        class_labels.contains(&"ConcreteService"),
        "Should find ConcreteService in new context, got: {:?}",
        class_labels
    );
    // The lightweight source scanner detects `abstract class` and
    // excludes it from new context.
    assert!(
        !class_labels.contains(&"AbstractService"),
        "Abstract stub should be excluded from new context, got: {:?}",
        class_labels
    );
}

/// After `new`, use-imported classes that are loaded as interfaces should
/// be excluded.
#[tokio::test]
async fn test_new_context_excludes_use_imported_interface() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{"autoload": {"psr-4": {"App\\": "src/"}}}"#,
        &[
            (
                "src/Contracts/Cacheable.php",
                "<?php\nnamespace App\\Contracts;\ninterface Cacheable {\n    public function cacheKey(): string;\n}\n",
            ),
            (
                "src/Models/CacheStore.php",
                "<?php\nnamespace App\\Models;\nclass CacheStore {}\n",
            ),
        ],
    );

    // Open both files so they are loaded into the ast_map.
    let iface_uri = Url::parse("file:///iface.php").unwrap();
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: iface_uri.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: "<?php\nnamespace App\\Contracts;\ninterface Cacheable {\n    public function cacheKey(): string;\n}\n".to_string(),
            },
        })
        .await;

    let class_uri = Url::parse("file:///cls.php").unwrap();
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: class_uri.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: "<?php\nnamespace App\\Models;\nclass CacheStore {}\n".to_string(),
            },
        })
        .await;

    let uri = Url::parse("file:///test_new_use_iface.php").unwrap();
    let text = concat!(
        "<?php\n",
        "use App\\Contracts\\Cacheable;\n",
        "use App\\Models\\CacheStore;\n",
        "new Cache\n",
    );

    let items = complete_at(&backend, &uri, text, 3, 9).await;
    let classes = class_items(&items);
    let class_fqns = fqns(&classes);

    assert!(
        class_fqns.contains(&"App\\Models\\CacheStore"),
        "Should include concrete use-imported class, got: {:?}",
        class_fqns
    );
    assert!(
        !class_fqns.contains(&"App\\Contracts\\Cacheable"),
        "Should exclude use-imported interface in `new` context, got: {:?}",
        class_fqns
    );
}

/// After `new`, class_index entries that are loaded as abstract should be
/// excluded.
#[tokio::test]
async fn test_new_context_excludes_class_index_abstract() {
    let backend = create_test_backend_with_stubs();

    // Load an abstract class into the ast_map.
    let abs_uri = Url::parse("file:///app/AbstractRepo.php").unwrap();
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: abs_uri.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: "<?php\nnamespace App;\nabstract class AbstractRepo {}\n".to_string(),
            },
        })
        .await;

    // Also put it in the class_index.
    {
        let mut idx = backend.fqn_uri_index().write();
        idx.insert("App\\AbstractRepo".to_string(), abs_uri.to_string());
    }

    let uri = Url::parse("file:///test_new_idx_abs.php").unwrap();
    let text = concat!("<?php\n", "new AbstractR\n",);

    let items = complete_at(&backend, &uri, text, 1, 13).await;
    let class_labels: Vec<&str> = class_items(&items)
        .iter()
        .map(|i| i.label.as_str())
        .collect();

    assert!(
        !class_labels.contains(&"App\\AbstractRepo"),
        "Should exclude class_index entry that is loaded as abstract, got: {:?}",
        class_labels
    );
}

// ─── FQN-prefix matching tests ─────────────────────────────────────────────

/// Typing `App\Models\U` should match classes whose FQN contains that
/// namespace path, using the FQN as label and insert text.
#[tokio::test]
async fn test_fqn_prefix_matches_by_namespace() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": {
                    "App\\": "src/"
                }
            }
        }"#,
        &[(
            "src/Models/User.php",
            concat!(
                "<?php\n",
                "namespace App\\Models;\n",
                "class User {\n",
                "    public function getName(): string { return ''; }\n",
                "}\n",
            ),
        )],
    );

    // Open the User file so it's in ast_map
    let user_uri = Url::parse(&format!(
        "file://{}",
        _dir.path().join("src/Models/User.php").display()
    ))
    .unwrap();
    let user_content = std::fs::read_to_string(_dir.path().join("src/Models/User.php")).unwrap();
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: user_uri,
                language_id: "php".to_string(),
                version: 1,
                text: user_content,
            },
        })
        .await;

    let uri = Url::parse("file:///fqn_prefix.php").unwrap();
    // Cursor after `App\Models\U` (12 chars on line 1, 0-indexed col 16)
    let text = concat!("<?php\n", "new App\\Models\\U\n",);

    let items = complete_at(&backend, &uri, text, 1, 16).await;
    let classes = class_items(&items);

    let user_item = classes
        .iter()
        .find(|i| i.detail.as_deref() == Some("App\\Models\\User"))
        .expect("Should find User via FQN prefix App\\Models\\U");

    // Label should be the FQN (not just the short name) in FQN mode.
    assert_eq!(
        user_item.label, "App\\Models\\User",
        "Label should be the FQN in FQN-prefix mode"
    );

    // text_edit should cover the entire typed prefix so the editor
    // replaces `App\Models\U` with the full FQN.
    assert!(
        user_item.text_edit.is_some(),
        "FQN-prefix completions should have a text_edit with explicit range"
    );
}

/// Typing `\App\Models\U` (with leading backslash) should match and
/// insert text with a leading backslash.
#[tokio::test]
async fn test_fqn_prefix_with_leading_backslash() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": {
                    "App\\": "src/"
                }
            }
        }"#,
        &[(
            "src/Models/User.php",
            concat!("<?php\n", "namespace App\\Models;\n", "class User {}\n",),
        )],
    );

    let user_uri = Url::parse(&format!(
        "file://{}",
        _dir.path().join("src/Models/User.php").display()
    ))
    .unwrap();
    let user_content = std::fs::read_to_string(_dir.path().join("src/Models/User.php")).unwrap();
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: user_uri,
                language_id: "php".to_string(),
                version: 1,
                text: user_content,
            },
        })
        .await;

    let uri = Url::parse("file:///fqn_leading.php").unwrap();
    let text = concat!("<?php\n", "new \\App\\Models\\U\n",);

    let items = complete_at(&backend, &uri, text, 1, 17).await;
    let classes = class_items(&items);

    let user_item = classes
        .iter()
        .find(|i| i.detail.as_deref() == Some("App\\Models\\User"))
        .expect("Should find User via FQN prefix \\App\\Models\\U");

    // insert_text should include the leading backslash.
    let insert = user_item.insert_text.as_deref().unwrap_or("");
    assert!(
        insert.starts_with("\\App\\Models\\User"),
        "insert_text should start with \\App\\Models\\User, got: {:?}",
        insert
    );
}

/// In FQN-prefix mode, no auto-import `use` statement should be added
/// because the user is explicitly typing the fully-qualified name.
#[tokio::test]
async fn test_fqn_prefix_skips_auto_import() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": {
                    "App\\": "src/"
                }
            }
        }"#,
        &[(
            "src/Models/User.php",
            concat!("<?php\n", "namespace App\\Models;\n", "class User {}\n",),
        )],
    );

    let user_uri = Url::parse(&format!(
        "file://{}",
        _dir.path().join("src/Models/User.php").display()
    ))
    .unwrap();
    let user_content = std::fs::read_to_string(_dir.path().join("src/Models/User.php")).unwrap();
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: user_uri,
                language_id: "php".to_string(),
                version: 1,
                text: user_content,
            },
        })
        .await;

    let uri = Url::parse("file:///fqn_noimport.php").unwrap();
    let text = concat!("<?php\n", "namespace Other;\n", "new App\\Models\\U\n",);

    let items = complete_at(&backend, &uri, text, 2, 16).await;
    let classes = class_items(&items);

    let user_item = classes
        .iter()
        .find(|i| i.detail.as_deref() == Some("App\\Models\\User"));

    if let Some(item) = user_item {
        assert!(
            item.additional_text_edits.is_none(),
            "FQN-prefix completions should NOT have additional_text_edits (auto-import), got: {:?}",
            item.additional_text_edits
        );
    }
}

/// In non-FQN mode (no `\` in the typed prefix), filter_text should be
/// the short name so the editor's fuzzy scorer ranks candidates by
/// short-name relevance rather than finding accidental substring hits
/// inside namespace paths.  The FQN remains visible in `label` and
/// `detail`.
///
/// In FQN mode (prefix contains `\`), filter_text should include the
/// full namespace path so the editor can drill into namespaces.
#[tokio::test]
async fn test_filter_text_uses_short_name_in_non_fqn_mode() {
    let backend = create_test_backend_with_stubs();

    let scaffolding_uri = Url::parse("file:///filter_scaffold.php").unwrap();
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: scaffolding_uri,
                language_id: "php".to_string(),
                version: 1,
                text: concat!(
                    "<?php\n",
                    "namespace Vendor\\Package;\n",
                    "class Widget {}\n",
                )
                .to_string(),
            },
        })
        .await;

    // ── Non-FQN mode: filter_text = short name ─────────────────
    let uri = Url::parse("file:///filter_test.php").unwrap();
    let text = concat!("<?php\n", "new Wid\n",);

    let items = complete_at(&backend, &uri, text, 1, 7).await;
    let classes = class_items(&items);

    let widget = find_by_fqn(&classes, "Vendor\\Package\\Widget")
        .expect("Should find Vendor\\Package\\Widget");

    let filter = widget.filter_text.as_deref().unwrap_or("");
    assert_eq!(
        filter, "Widget",
        "non-FQN filter_text should be the short name, got: {:?}",
        filter
    );

    // ── FQN mode: filter_text = full path ──────────────────────
    let uri_fqn = Url::parse("file:///filter_fqn_test.php").unwrap();
    let text_fqn = concat!("<?php\n", "new Vendor\\Pack\n",);

    let items_fqn = complete_at(&backend, &uri_fqn, text_fqn, 1, 15).await;
    let classes_fqn = class_items(&items_fqn);

    let widget_fqn = classes_fqn
        .iter()
        .find(|i| i.label == "Vendor\\Package\\Widget")
        .expect("Should find Vendor\\Package\\Widget in FQN mode");

    let filter_fqn = widget_fqn.filter_text.as_deref().unwrap_or("");
    assert!(
        filter_fqn.contains("Vendor\\Package"),
        "FQN-mode filter_text should include the namespace path, got: {:?}",
        filter_fqn
    );
}

/// The text_edit replacement range should cover the entire typed prefix
/// so that `http\En` is fully replaced with the selected FQN, not
/// appended after the last `\`.
#[tokio::test]
async fn test_fqn_prefix_text_edit_replaces_full_prefix() {
    let backend = create_test_backend_with_stubs();

    let scaffolding_uri = Url::parse("file:///textedit_scaffold.php").unwrap();
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: scaffolding_uri,
                language_id: "php".to_string(),
                version: 1,
                text: concat!(
                    "<?php\n",
                    "namespace http\\Exception;\n",
                    "class BadUrlException {}\n",
                )
                .to_string(),
            },
        })
        .await;

    let uri = Url::parse("file:///textedit_test.php").unwrap();
    // User typed `http\Ex` (7 chars) starting at col 22
    let text = concat!("<?php\n", "if ($user instanceof http\\Ex) {}\n",);

    let items = complete_at(&backend, &uri, text, 1, 28).await;
    let classes = class_items(&items);

    let exc_item = classes
        .iter()
        .find(|i| i.detail.as_deref() == Some("http\\Exception\\BadUrlException"))
        .expect("Should find BadUrlException via FQN prefix http\\Ex");

    // Must have a text_edit that covers the full prefix `http\Ex`.
    let te = exc_item
        .text_edit
        .as_ref()
        .expect("FQN-prefix completions must have a text_edit");

    match te {
        CompletionTextEdit::Edit(edit) => {
            // The prefix `http\Ex` is 7 characters, starting at col 21.
            assert_eq!(
                edit.range.start,
                Position {
                    line: 1,
                    character: 21,
                },
                "text_edit range should start where the FQN prefix begins"
            );
            assert_eq!(
                edit.range.end,
                Position {
                    line: 1,
                    character: 28,
                },
                "text_edit range should end at the cursor"
            );
            assert!(
                edit.new_text.contains("http\\Exception\\BadUrlException"),
                "text_edit new_text should be the full FQN, got: {:?}",
                edit.new_text
            );
        }
        _ => panic!("Expected CompletionTextEdit::Edit"),
    }
}

/// When the user types `\Demo\` in namespace `Demo` and picks `Demo\Box`,
/// the insert text should be simplified to `Box` (not `\Demo\Box` or `\Box`)
/// because the class is already in the current namespace.
#[tokio::test]
async fn test_fqn_prefix_same_namespace_simplifies_to_short_name() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": {
                    "Demo\\": "src/"
                }
            }
        }"#,
        &[(
            "src/Box.php",
            concat!("<?php\n", "namespace Demo;\n", "class Box {}\n",),
        )],
    );

    // Open the Box file so it's in ast_map.
    let box_uri = Url::parse(&format!(
        "file://{}",
        _dir.path().join("src/Box.php").display()
    ))
    .unwrap();
    let box_content = std::fs::read_to_string(_dir.path().join("src/Box.php")).unwrap();
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: box_uri,
                language_id: "php".to_string(),
                version: 1,
                text: box_content,
            },
        })
        .await;

    let uri = Url::parse("file:///fqn_same_ns.php").unwrap();
    // We are in namespace Demo, typing `\Demo\` (6 chars, col 25).
    let text = concat!(
        "<?php\n",
        "namespace Demo;\n",
        "if ($user instanceof \\Demo\\) {}\n",
    );

    let items = complete_at(&backend, &uri, text, 2, 27).await;
    let classes = class_items(&items);

    let box_item = classes
        .iter()
        .find(|i| i.detail.as_deref() == Some("Demo\\Box"))
        .expect("Should find Box via FQN prefix \\Demo\\");

    // The label should be the FQN.
    assert_eq!(box_item.label, "Demo\\Box", "Label should be the FQN");

    // The text_edit should replace `\Demo\` with just `Box`.
    let te = box_item
        .text_edit
        .as_ref()
        .expect("FQN-prefix completions should have a text_edit");
    match te {
        CompletionTextEdit::Edit(edit) => {
            assert_eq!(
                edit.new_text, "Box",
                "text_edit should insert 'Box', not the full FQN"
            );
        }
        _ => panic!("Expected CompletionTextEdit::Edit"),
    }
}

/// When the user types `\Other\` in namespace `Demo` and picks `Other\Foo`,
/// the full FQN should be preserved (not simplified).
#[tokio::test]
async fn test_fqn_prefix_different_namespace_keeps_fqn() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": {
                    "Other\\": "src/"
                }
            }
        }"#,
        &[(
            "src/Foo.php",
            concat!("<?php\n", "namespace Other;\n", "class Foo {}\n",),
        )],
    );

    let foo_uri = Url::parse(&format!(
        "file://{}",
        _dir.path().join("src/Foo.php").display()
    ))
    .unwrap();
    let foo_content = std::fs::read_to_string(_dir.path().join("src/Foo.php")).unwrap();
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: foo_uri,
                language_id: "php".to_string(),
                version: 1,
                text: foo_content,
            },
        })
        .await;

    let uri = Url::parse("file:///fqn_diff_ns.php").unwrap();
    // We are in namespace Demo, typing `\Other\` (7 chars).
    let text = concat!(
        "<?php\n",
        "namespace Demo;\n",
        "if ($user instanceof \\Other\\) {}\n",
    );

    let items = complete_at(&backend, &uri, text, 2, 28).await;
    let classes = class_items(&items);

    let foo_item = classes
        .iter()
        .find(|i| i.detail.as_deref() == Some("Other\\Foo"))
        .expect("Should find Foo via FQN prefix \\Other\\");

    // Label should be the full FQN since it's not in the current namespace.
    assert_eq!(
        foo_item.label, "Other\\Foo",
        "Label should be the full FQN when class is in a different namespace"
    );

    let te = foo_item
        .text_edit
        .as_ref()
        .expect("FQN-prefix completions should have a text_edit");
    match te {
        CompletionTextEdit::Edit(edit) => {
            assert!(
                edit.new_text.contains("\\Other\\Foo"),
                "text_edit should insert the full FQN with leading backslash, got: {:?}",
                edit.new_text
            );
        }
        _ => panic!("Expected CompletionTextEdit::Edit"),
    }
}

/// `namespace ` completion should suggest known namespace names, not class names.
#[tokio::test]
async fn test_namespace_declaration_suggests_namespaces() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": {
                    "App\\": "src/"
                }
            }
        }"#,
        &[
            (
                "src/Models/User.php",
                concat!("<?php\n", "namespace App\\Models;\n", "class User {}\n",),
            ),
            (
                "src/Services/AuthService.php",
                concat!(
                    "<?php\n",
                    "namespace App\\Services;\n",
                    "class AuthService {}\n",
                ),
            ),
        ],
    );

    // Open one file so its namespace appears in namespace_map.
    let user_uri = Url::parse(&format!(
        "file://{}",
        _dir.path().join("src/Models/User.php").display()
    ))
    .unwrap();
    let user_content = std::fs::read_to_string(_dir.path().join("src/Models/User.php")).unwrap();
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: user_uri,
                language_id: "php".to_string(),
                version: 1,
                text: user_content,
            },
        })
        .await;

    let uri = Url::parse("file:///ns_decl.php").unwrap();
    let text = concat!("<?php\n", "namespace App\n",);

    let items = complete_at(&backend, &uri, text, 1, 13).await;
    let all_labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();

    // Should include namespace names that are under the PSR-4 prefix.
    assert!(
        all_labels.iter().any(|l| l.contains("App\\Models")),
        "Should suggest App\\Models namespace, got labels: {:?}",
        all_labels
    );

    // Should NOT include class names like "User".
    assert!(
        !all_labels.contains(&"User"),
        "Should NOT suggest class names in namespace context, got labels: {:?}",
        all_labels
    );

    // Should NOT include stub namespaces that are outside PSR-4 prefixes.
    assert!(
        !all_labels.contains(&"Decimal"),
        "Should NOT suggest stub-only namespaces outside PSR-4 prefixes, got labels: {:?}",
        all_labels
    );

    // Items should have MODULE kind.
    for item in &items {
        assert_eq!(
            item.kind,
            Some(CompletionItemKind::MODULE),
            "Namespace items should have MODULE kind, got {:?} for {:?}",
            item.kind,
            item.label
        );
    }
}

/// `namespace ` completion should discover sub-namespaces from cached files
/// that fall under a PSR-4 prefix.
#[tokio::test]
async fn test_namespace_declaration_discovers_cached_sub_namespaces() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": {
                    "App\\": "src/"
                }
            }
        }"#,
        &[
            (
                "src/Models/User.php",
                concat!("<?php\n", "namespace App\\Models;\n", "class User {}\n",),
            ),
            (
                "src/Models/Concerns/HasUuids.php",
                concat!(
                    "<?php\n",
                    "namespace App\\Models\\Concerns;\n",
                    "trait HasUuids {}\n",
                ),
            ),
        ],
    );

    // Open the Concerns file so its namespace enters namespace_map.
    let concerns_uri = Url::parse(&format!(
        "file://{}",
        _dir.path()
            .join("src/Models/Concerns/HasUuids.php")
            .display()
    ))
    .unwrap();
    let concerns_content =
        std::fs::read_to_string(_dir.path().join("src/Models/Concerns/HasUuids.php")).unwrap();
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: concerns_uri,
                language_id: "php".to_string(),
                version: 1,
                text: concerns_content,
            },
        })
        .await;

    let uri = Url::parse("file:///ns_subdir.php").unwrap();
    let text = concat!("<?php\n", "namespace App\\Models\n",);

    let items = complete_at(&backend, &uri, text, 1, 20).await;
    let all_labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();

    assert!(
        all_labels.contains(&"App\\Models\\Concerns"),
        "Should discover sub-namespace from cached files, got labels: {:?}",
        all_labels
    );
}

/// `namespace` inside a class body (e.g. `namespace\func()`) should NOT
/// produce namespace-declaration completions.  It should fall through to
/// normal class/function completion instead of MODULE items.
#[tokio::test]
async fn test_namespace_context_not_inside_class_body() {
    let backend = create_test_backend_with_stubs();

    let uri = Url::parse("file:///ns_in_body.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Foo {\n",
        "    public function bar() {\n",
        "        namespace\n",
        "    }\n",
        "}\n",
    );

    // Cursor at end of `namespace` on line 3.
    let items = complete_at(&backend, &uri, text, 3, 17).await;

    // If this were incorrectly detected as NamespaceDeclaration, all
    // items would have MODULE kind.  Verify that at least some items
    // have a different kind (CLASS, FUNCTION, etc.).
    let has_non_module = items
        .iter()
        .any(|i| i.kind != Some(CompletionItemKind::MODULE));
    assert!(
        items.is_empty() || has_non_module,
        "`namespace` inside a class body should NOT produce only MODULE completions"
    );
}

/// When the user types `\Demo` (leading backslash, single segment) in
/// namespace `Demo` and picks `Demo\Box`, the result should be `Box`
/// (not `\Box`).  The leading `\` activates FQN mode, and the same-
/// namespace check simplifies the reference.
#[tokio::test]
async fn test_fqn_leading_backslash_single_segment_same_namespace() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": {
                    "Demo\\": "src/"
                }
            }
        }"#,
        &[(
            "src/Box.php",
            concat!("<?php\n", "namespace Demo;\n", "class Box {}\n",),
        )],
    );

    let box_uri = Url::parse(&format!(
        "file://{}",
        _dir.path().join("src/Box.php").display()
    ))
    .unwrap();
    let box_content = std::fs::read_to_string(_dir.path().join("src/Box.php")).unwrap();
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: box_uri,
                language_id: "php".to_string(),
                version: 1,
                text: box_content,
            },
        })
        .await;

    let uri = Url::parse("file:///fqn_backslash_single.php").unwrap();
    // In namespace Demo, typing `\Demo` (5 chars starting at col 21).
    let text = concat!(
        "<?php\n",
        "namespace Demo;\n",
        "if ($user instanceof \\Demo) {}\n",
    );

    let items = complete_at(&backend, &uri, text, 2, 26).await;
    let classes = class_items(&items);

    let box_item = classes
        .iter()
        .find(|i| i.detail.as_deref() == Some("Demo\\Box"))
        .expect("Should find Box via prefix \\Demo");

    // The text_edit should replace `\Demo` with just `Box`.
    let te = box_item
        .text_edit
        .as_ref()
        .expect("Leading-backslash completions should have a text_edit");
    match te {
        CompletionTextEdit::Edit(edit) => {
            assert_eq!(
                edit.new_text, "Box",
                "text_edit should insert 'Box' (same namespace), not '\\Box' or '\\Demo\\Box'"
            );
        }
        _ => panic!("Expected CompletionTextEdit::Edit"),
    }
}

/// `use function` completions should NOT include parentheses and should
/// end with a semicolon so the statement is complete.
#[tokio::test]
async fn test_use_function_no_parentheses() {
    let backend = create_test_backend_with_function_stubs();

    let uri = Url::parse("file:///use_func_parens.php").unwrap();
    let text = concat!("<?php\n", "use function array_ma\n",);

    let items = complete_at(&backend, &uri, text, 1, 21).await;

    let func_items: Vec<&CompletionItem> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::FUNCTION))
        .collect();

    assert!(
        !func_items.is_empty(),
        "Should have function completions for 'array_ma'"
    );

    for item in &func_items {
        let insert = item.insert_text.as_deref().unwrap_or(&item.label);
        assert!(
            !insert.contains('('),
            "use function completions should NOT contain parentheses, got insert_text: {:?} for {:?}",
            insert,
            item.label
        );
        assert!(
            insert.ends_with(';'),
            "use function completions should end with ';', got insert_text: {:?} for {:?}",
            insert,
            item.label
        );
        assert!(
            item.insert_text_format != Some(InsertTextFormat::SNIPPET),
            "use function completions should be plain text, not snippets, for {:?}",
            item.label
        );
    }
}

/// `use const` completions should end with a semicolon.
#[tokio::test]
async fn test_use_const_semicolon_termination() {
    let backend = create_test_backend_with_stubs();

    {
        let mut dmap = backend.global_defines().write();
        dmap.insert(
            "MY_CONST".to_string(),
            phpantom_lsp::DefineInfo {
                file_uri: "file:///defs.php".to_string(),
                name_offset: 0,
                value: None,
            },
        );
    }

    let uri = Url::parse("file:///use_const_semi.php").unwrap();
    let text = concat!("<?php\n", "use const MY_C\n",);

    let items = complete_at(&backend, &uri, text, 1, 14).await;

    let const_items: Vec<&CompletionItem> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::CONSTANT))
        .collect();

    assert!(
        !const_items.is_empty(),
        "Should have constant completions for 'MY_C'"
    );

    for item in &const_items {
        let insert = item.insert_text.as_deref().unwrap_or(&item.label);
        assert!(
            insert.ends_with(';'),
            "use const completions should end with ';', got insert_text: {:?} for {:?}",
            insert,
            item.label
        );
    }
}

/// `use` (class import) completions should end with a semicolon, but the
/// `function` / `const` keyword hints should NOT (they continue the statement).
#[tokio::test]
async fn test_use_class_import_semicolon_termination() {
    let backend = create_test_backend_with_stubs();

    // Register a class so that `use DateT` has something to complete.
    let scaffold_uri = Url::parse("file:///scaffold_datetime.php").unwrap();
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: scaffold_uri,
                language_id: "php".to_string(),
                version: 1,
                text: "<?php\nnamespace App;\nclass DateTransformer {}\n".to_string(),
            },
        })
        .await;

    let uri = Url::parse("file:///use_class_semi.php").unwrap();
    let text = concat!("<?php\n", "use DateT\n",);

    let items = complete_at(&backend, &uri, text, 1, 9).await;

    let class_items: Vec<&CompletionItem> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::CLASS))
        .collect();

    assert!(
        !class_items.is_empty(),
        "Should have class completions for 'DateT', got: {:?}",
        labels(&items)
    );

    for item in &class_items {
        let insert = item.insert_text.as_deref().unwrap_or(&item.label);
        assert!(
            insert.ends_with(';'),
            "use class completions should end with ';', got insert_text: {:?} for {:?}",
            insert,
            item.label
        );
    }

    // The `function` / `const` keyword hints should NOT have semicolons
    // because they continue the statement (e.g. `use function `).
    let keyword_items: Vec<&CompletionItem> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::KEYWORD))
        .collect();
    for item in &keyword_items {
        let insert = item.insert_text.as_deref().unwrap_or(&item.label);
        assert!(
            !insert.ends_with(';'),
            "keyword hints should NOT end with ';', got insert_text: {:?} for {:?}",
            insert,
            item.label
        );
    }
}

/// Namespace declaration completion should exclude namespaces that are
/// not under any PSR-4 prefix (e.g. stub-only namespaces like `Decimal`).
#[tokio::test]
async fn test_namespace_declaration_excludes_non_psr4_namespaces() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": {
                    "App\\": "src/"
                }
            }
        }"#,
        &[(
            "src/Models/User.php",
            concat!("<?php\n", "namespace App\\Models;\n", "class User {}\n",),
        )],
    );

    // Open User so its namespace enters namespace_map.
    let user_uri = Url::parse(&format!(
        "file://{}",
        _dir.path().join("src/Models/User.php").display()
    ))
    .unwrap();
    let user_content = std::fs::read_to_string(_dir.path().join("src/Models/User.php")).unwrap();
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: user_uri,
                language_id: "php".to_string(),
                version: 1,
                text: user_content,
            },
        })
        .await;

    // Also inject a class_index entry for a non-PSR-4 namespace.
    {
        let mut idx = backend.fqn_uri_index().write();
        idx.insert(
            "MySql\\Enums\\IntTypes".to_string(),
            "file:///somewhere.php".to_string(),
        );
    }

    let uri = Url::parse("file:///ns_filter.php").unwrap();
    let text = concat!("<?php\n", "namespace My\n",);

    let items = complete_at(&backend, &uri, text, 1, 12).await;
    let all_labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();

    // MySql is NOT under the App\ PSR-4 prefix, so it should be excluded.
    assert!(
        !all_labels.iter().any(|l| l.contains("MySql")),
        "Should NOT suggest namespaces outside PSR-4 prefixes, got labels: {:?}",
        all_labels
    );
}

/// Namespace completion should only include PSR-4 prefixes and cached
/// namespaces under those prefixes, with each level exploded.
#[tokio::test]
async fn test_namespace_declaration_psr4_and_cached_only() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": {
                    "JohnyDogood\\": "src/",
                    "JUtils\\": "utils/"
                }
            }
        }"#,
        &[
            (
                "src/Money/USD.php",
                concat!(
                    "<?php\n",
                    "namespace JohnyDogood\\Money;\n",
                    "class USD {}\n",
                ),
            ),
            ("utils/.gitkeep", ""),
        ],
    );

    // Open the USD file so its namespace is cached.
    let usd_uri = Url::parse(&format!(
        "file://{}",
        _dir.path().join("src/Money/USD.php").display()
    ))
    .unwrap();
    let usd_content = std::fs::read_to_string(_dir.path().join("src/Money/USD.php")).unwrap();
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: usd_uri,
                language_id: "php".to_string(),
                version: 1,
                text: usd_content,
            },
        })
        .await;

    // Also inject a class_index entry for a class outside PSR-4.
    {
        let mut idx = backend.fqn_uri_index().write();
        idx.insert(
            "MySql\\Enums\\IntTypes".to_string(),
            "file:///ext.php".to_string(),
        );
    }

    let uri = Url::parse("file:///ns_psr4.php").unwrap();
    let text = concat!("<?php\n", "namespace J\n",);

    let items = complete_at(&backend, &uri, text, 1, 11).await;
    let all_labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();

    // PSR-4 prefixes should be present.
    assert!(
        all_labels.contains(&"JohnyDogood"),
        "Should suggest PSR-4 prefix JohnyDogood, got: {:?}",
        all_labels
    );
    assert!(
        all_labels.contains(&"JUtils"),
        "Should suggest PSR-4 prefix JUtils, got: {:?}",
        all_labels
    );

    // Cached sub-namespace under PSR-4 prefix should be present.
    assert!(
        all_labels.contains(&"JohnyDogood\\Money"),
        "Should suggest cached sub-namespace JohnyDogood\\Money, got: {:?}",
        all_labels
    );

    // Class basename should NOT appear.
    assert!(
        !all_labels.contains(&"USD"),
        "Should NOT suggest class names, got: {:?}",
        all_labels
    );

    // Non-PSR-4 namespace should NOT appear.
    assert!(
        !all_labels.iter().any(|l| l.contains("MySql")),
        "Should NOT suggest non-PSR-4 namespaces, got: {:?}",
        all_labels
    );
}

/// When typing `namespace Tests\Feature\D` and picking `Tests\Feature\Domain`,
/// the text_edit must replace the entire typed prefix (`Tests\Feature\D`) so
/// the result is `namespace Tests\Feature\Domain;` — not the doubled
/// `namespace Tests\Feature\Tests\Feature\Domain;`.
#[tokio::test]
async fn test_namespace_declaration_replaces_full_prefix() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": {
                    "Tests\\": "tests/"
                }
            }
        }"#,
        &[(
            "tests/Feature/Domain/SomeTest.php",
            concat!(
                "<?php\n",
                "namespace Tests\\Feature\\Domain;\n",
                "class SomeTest {}\n",
            ),
        )],
    );

    // Open the file so its namespace enters namespace_map / ast_map.
    let file_uri = Url::parse(&format!(
        "file://{}",
        _dir.path()
            .join("tests/Feature/Domain/SomeTest.php")
            .display()
    ))
    .unwrap();
    let file_content =
        std::fs::read_to_string(_dir.path().join("tests/Feature/Domain/SomeTest.php")).unwrap();
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: file_uri,
                language_id: "php".to_string(),
                version: 1,
                text: file_content,
            },
        })
        .await;

    let uri = Url::parse("file:///ns_replace.php").unwrap();
    // Typing `namespace Tests\Feature\D` — cursor at col 27.
    let text = concat!("<?php\n", "namespace Tests\\Feature\\D\n",);

    let items = complete_at(&backend, &uri, text, 1, 25).await;

    let domain_item = items
        .iter()
        .find(|i| i.label == "Tests\\Feature\\Domain")
        .expect("Should find Tests\\Feature\\Domain in namespace completions");

    // The item MUST carry a text_edit that replaces the full typed prefix.
    let te = domain_item
        .text_edit
        .as_ref()
        .expect("Namespace completions with backslash should have a text_edit");
    match te {
        CompletionTextEdit::Edit(edit) => {
            assert_eq!(
                edit.new_text, "Tests\\Feature\\Domain",
                "text_edit should insert the full namespace"
            );
            // The range should start at the beginning of the typed prefix
            // (col 10, right after `namespace `).
            assert_eq!(
                edit.range.start,
                Position {
                    line: 1,
                    character: 10
                },
                "replacement range should start at the beginning of the typed prefix"
            );
            assert_eq!(
                edit.range.end,
                Position {
                    line: 1,
                    character: 25
                },
                "replacement range should end at the cursor"
            );
        }
        _ => panic!("Expected CompletionTextEdit::Edit"),
    }
}

/// When the user has `use Cassandra\Exception;` and types `Exception\AlreadyEx`,
/// picking `Cassandra\Exception\AlreadyExistsException` should insert
/// `Exception\AlreadyExistsException` (shortened via the use-map prefix),
/// not the full FQN.
#[tokio::test]
async fn test_fqn_shortened_via_use_map_prefix() {
    let backend = create_test_backend_with_stubs();

    // Put the class in class_index so it appears in completions.
    {
        let mut idx = backend.fqn_uri_index().write();
        idx.insert(
            "Cassandra\\Exception\\AlreadyExistsException".to_string(),
            "file:///vendor/cassandra.php".to_string(),
        );
    }

    let uri = Url::parse("file:///shorten_prefix.php").unwrap();
    let text = concat!(
        "<?php\n",
        "use Cassandra\\Exception;\n",
        "if ($user instanceof Exception\\AlreadyEx) {}\n",
    );

    // Cursor at end of `AlreadyEx` on line 2 (col 40).
    let items = complete_at(&backend, &uri, text, 2, 40).await;
    let cls = class_items(&items);

    let item = cls
        .iter()
        .find(|i| i.detail.as_deref() == Some("Cassandra\\Exception\\AlreadyExistsException"))
        .expect("Should find AlreadyExistsException in completions");

    // The label should be the full FQN.
    assert_eq!(
        item.label, "Cassandra\\Exception\\AlreadyExistsException",
        "label should be the full FQN"
    );

    // The text_edit should insert the shortened form.
    let te = item
        .text_edit
        .as_ref()
        .expect("FQN completions should have a text_edit");
    match te {
        CompletionTextEdit::Edit(edit) => {
            assert_eq!(
                edit.new_text, "Exception\\AlreadyExistsException",
                "text_edit should insert the shortened form"
            );
        }
        _ => panic!("Expected CompletionTextEdit::Edit"),
    }

    // No additional use statement should be generated.
    assert!(
        item.additional_text_edits.is_none()
            || item.additional_text_edits.as_ref().unwrap().is_empty(),
        "should not generate a use import when already reachable via existing import"
    );
}

/// When the user has `use Cassandra\Exception\AlreadyExistsException;` and
/// types `\Cassa`, picking the class should insert just
/// `AlreadyExistsException` (the short imported name) rather than the FQN.
#[tokio::test]
async fn test_fqn_shortened_via_use_map_exact_match_leading_backslash() {
    let backend = create_test_backend_with_stubs();

    {
        let mut idx = backend.fqn_uri_index().write();
        idx.insert(
            "Cassandra\\Exception\\AlreadyExistsException".to_string(),
            "file:///vendor/cassandra.php".to_string(),
        );
    }

    let uri = Url::parse("file:///shorten_exact.php").unwrap();
    let text = concat!(
        "<?php\n",
        "use Cassandra\\Exception\\AlreadyExistsException;\n",
        "if ($user instanceof \\Cassa) {}\n",
    );

    // Cursor at end of `\Cassa` on line 2 (col 27).
    let items = complete_at(&backend, &uri, text, 2, 27).await;
    let cls = class_items(&items);

    let item = cls
        .iter()
        .find(|i| i.detail.as_deref() == Some("Cassandra\\Exception\\AlreadyExistsException"))
        .expect("Should find AlreadyExistsException in completions");

    // The label should be the full FQN.
    assert_eq!(
        item.label, "Cassandra\\Exception\\AlreadyExistsException",
        "label should be the full FQN"
    );

    // The text_edit should replace `\Cassa` with just the short name.
    let te = item
        .text_edit
        .as_ref()
        .expect("FQN completions should have a text_edit");
    match te {
        CompletionTextEdit::Edit(edit) => {
            assert_eq!(
                edit.new_text, "AlreadyExistsException",
                "text_edit should insert the short imported name, not the FQN"
            );
        }
        _ => panic!("Expected CompletionTextEdit::Edit"),
    }
}

/// Use-map shortening should NOT apply in `use` import context —
/// the user is writing a `use` statement and needs the full FQN.
#[tokio::test]
async fn test_use_import_context_does_not_shorten() {
    let backend = create_test_backend_with_stubs();

    {
        let mut idx = backend.fqn_uri_index().write();
        idx.insert(
            "Cassandra\\Exception\\AlreadyExistsException".to_string(),
            "file:///vendor/cassandra.php".to_string(),
        );
    }

    let uri = Url::parse("file:///use_no_shorten.php").unwrap();
    let text = concat!(
        "<?php\n",
        "use Cassandra\\Exception;\n",
        "use Cassandra\\Exception\\Already\n",
    );

    // Cursor at end of `Already` on line 2.
    let items = complete_at(&backend, &uri, text, 2, 31).await;
    let cls = class_items(&items);

    let item = cls
        .iter()
        .find(|i| i.detail.as_deref() == Some("Cassandra\\Exception\\AlreadyExistsException"))
        .expect("Should find AlreadyExistsException in use-import completions");

    // In use-import context, the label should be the full FQN.
    assert_eq!(
        item.label, "Cassandra\\Exception\\AlreadyExistsException",
        "use-import context should NOT shorten via use-map"
    );
}

/// A `use` statement that imports a namespace (not a class) should NOT
/// produce a phantom class completion item.  E.g. `use Luxplus\Core\Enums as LCE;`
/// where `Enums` is a namespace containing enum classes, not a class itself.
#[tokio::test]
async fn test_namespace_alias_import_not_shown_as_class() {
    let backend = create_test_backend_with_stubs();

    // Register classes UNDER the namespace so the LSP knows it's a namespace.
    {
        let mut idx = backend.fqn_uri_index().write();
        idx.insert(
            "Luxplus\\Core\\Enums\\Status".to_string(),
            "file:///vendor/luxplus/enums/Status.php".to_string(),
        );
        idx.insert(
            "Luxplus\\Core\\Enums\\Color".to_string(),
            "file:///vendor/luxplus/enums/Color.php".to_string(),
        );
    }

    // Use prefix "LCE" which matches the alias for the namespace import.
    let uri = Url::parse("file:///ns_alias.php").unwrap();
    let text = concat!("<?php\n", "use Luxplus\\Core\\Enums as LCE;\n", "new LCE\n",);

    let items = complete_at(&backend, &uri, text, 2, 7).await;
    let cls = class_items(&items);

    // `Luxplus\Core\Enums` is a namespace, not a class — it should NOT
    // appear as a completion item.
    let phantom = cls
        .iter()
        .find(|i| i.detail.as_deref() == Some("Luxplus\\Core\\Enums"));
    assert!(
        phantom.is_none(),
        "Namespace alias should not appear as a class completion, got: {:?}",
        phantom
    );
}

/// Classes under a namespace-aliased import should still appear when
/// the typed prefix matches their short name.
#[tokio::test]
async fn test_classes_under_namespace_alias_still_available() {
    let backend = create_test_backend_with_stubs();

    {
        let mut idx = backend.fqn_uri_index().write();
        idx.insert(
            "Luxplus\\Core\\Enums\\Status".to_string(),
            "file:///vendor/luxplus/enums/Status.php".to_string(),
        );
    }

    let uri = Url::parse("file:///ns_alias_child.php").unwrap();
    let text = concat!(
        "<?php\n",
        "use Luxplus\\Core\\Enums as LCE;\n",
        "new Stat\n",
    );

    let items = complete_at(&backend, &uri, text, 2, 8).await;
    let cls = class_items(&items);

    let status = cls
        .iter()
        .find(|i| i.detail.as_deref() == Some("Luxplus\\Core\\Enums\\Status"));
    assert!(
        status.is_some(),
        "Classes under the namespace should still appear in completions"
    );
}

/// A `use` import for a class that hasn't been discovered yet should
/// still appear in completions (benefit of the doubt).
#[tokio::test]
async fn test_undiscovered_use_import_still_shown() {
    let backend = create_test_backend_with_stubs();

    // Register the class in fqn_uri_index so it is discoverable.
    {
        let mut idx = backend.fqn_uri_index().write();
        idx.insert(
            "Vendor\\SomeLibrary\\Widget".to_string(),
            "file:///vendor/Widget.php".to_string(),
        );
    }

    let uri = Url::parse("file:///undiscovered.php").unwrap();
    let text = concat!("<?php\n", "use Vendor\\SomeLibrary\\Widget;\n", "new Wid\n",);

    let items = complete_at(&backend, &uri, text, 2, 7).await;
    let cls = class_items(&items);
    let class_fqns = fqns(&cls);

    // The imported class should appear even though it's not in any index.
    assert!(
        class_fqns.contains(&"Vendor\\SomeLibrary\\Widget"),
        "Undiscovered use-imported class should still appear, got: {:?}",
        class_fqns
    );
}

// ─── Use-import conflict resolution ────────────────────────────────────────

/// When the file already has `use Cassandra\Exception;` and a class_index
/// class `App\Exception` is suggested, the LSP must NOT insert a second
/// `use ... Exception;`.  Instead it should insert `\App\Exception` at
/// the usage site.
#[tokio::test]
async fn test_conflicting_use_import_class_index_falls_back_to_fqn() {
    let backend = create_test_backend_with_stubs();

    {
        let mut idx = backend.fqn_uri_index().write();
        idx.insert(
            "App\\Exception".to_string(),
            "file:///app/Exception.php".to_string(),
        );
    }

    let uri = Url::parse("file:///controller.php").unwrap();
    let text = concat!(
        "<?php\n",
        "namespace App\\Controllers;\n",
        "use Cassandra\\Exception;\n",
        "\n",
        "new Exc\n",
    );

    let items = complete_at(&backend, &uri, text, 4, 7).await;
    let app_exc = items
        .iter()
        .find(|i| i.detail.as_deref() == Some("App\\Exception"))
        .expect("Should have App\\Exception completion");

    // Must NOT have additional_text_edits (no `use` statement).
    assert!(
        app_exc.additional_text_edits.is_none(),
        "Conflicting import should not produce a use statement, got: {:?}",
        app_exc.additional_text_edits
    );

    // Insert text should be the FQN with leading backslash.
    let insert = app_exc.insert_text.as_deref().unwrap_or("");
    assert!(
        insert.starts_with("\\App\\Exception"),
        "Should insert FQN with leading backslash, got: {:?}",
        insert
    );
}

/// Same conflict scenario but with the class coming from the classmap.
#[tokio::test]
async fn test_conflicting_use_import_classmap_falls_back_to_fqn() {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    fs::write(
        dir.path().join("composer.json"),
        r#"{"name": "test/project"}"#,
    )
    .expect("failed to write composer.json");

    let composer_dir = dir.path().join("vendor").join("composer");
    fs::create_dir_all(&composer_dir).expect("failed to create vendor/composer");
    fs::write(
        composer_dir.join("autoload_classmap.php"),
        concat!(
            "<?php\n",
            "$vendorDir = dirname(__DIR__);\n",
            "return array(\n",
            "    'App\\\\Exception' => $vendorDir . '/app/Exception.php',\n",
            ");\n",
        ),
    )
    .expect("failed to write autoload_classmap.php");

    let backend = Backend::new_test_with_workspace(dir.path().to_path_buf(), vec![]);
    let classmap = parse_autoload_classmap(dir.path(), "vendor");
    {
        let mut idx = backend.fqn_uri_index().write();
        for (fqn, path) in &classmap {
            idx.insert(fqn.clone(), Url::from_file_path(path).unwrap().to_string());
        }
    }

    let uri = Url::parse("file:///controller.php").unwrap();
    let text = concat!(
        "<?php\n",
        "namespace App\\Controllers;\n",
        "use Cassandra\\Exception;\n",
        "\n",
        "new Exc\n",
    );

    let items = complete_at(&backend, &uri, text, 4, 7).await;
    let app_exc = items
        .iter()
        .find(|i| i.detail.as_deref() == Some("App\\Exception"))
        .expect("Should have App\\Exception completion from classmap");

    assert!(
        app_exc.additional_text_edits.is_none(),
        "Conflicting classmap import should not produce a use statement, got: {:?}",
        app_exc.additional_text_edits
    );

    let insert = app_exc.insert_text.as_deref().unwrap_or("");
    assert!(
        insert.starts_with("\\App\\Exception"),
        "Classmap conflict should insert FQN with leading backslash, got: {:?}",
        insert
    );
}

/// When a stub class short name conflicts with an existing import, the
/// stub completion should also fall back to FQN.
#[tokio::test]
async fn test_conflicting_use_import_stub_falls_back_to_fqn() {
    // Register a stub class called "Exception".
    let mut stubs: HashMap<&'static str, &'static str> = HashMap::new();
    stubs.insert("Exception", "<?php\nclass Exception {}\n");
    let backend = Backend::new_test_with_stubs(stubs);

    let uri = Url::parse("file:///controller.php").unwrap();
    let text = concat!(
        "<?php\n",
        "namespace App\\Controllers;\n",
        "use Cassandra\\Exception;\n",
        "\n",
        "new Exc\n",
    );

    let items = complete_at(&backend, &uri, text, 4, 7).await;
    let stub_exc = items
        .iter()
        .find(|i| i.detail.as_deref() == Some("Exception") && i.label != "Exception")
        .or_else(|| {
            // The label might stay "Exception" — look for the one that
            // has FQN insert text and no additional edits.
            items.iter().find(|i| {
                i.detail.as_deref() == Some("Exception")
                    && i.additional_text_edits.is_none()
                    && i.insert_text
                        .as_deref()
                        .is_some_and(|t| t.starts_with("\\Exception"))
            })
        })
        .expect("Should have stub Exception completion with FQN fallback");

    assert!(
        stub_exc.additional_text_edits.is_none(),
        "Conflicting stub import should not produce a use statement"
    );

    let insert = stub_exc.insert_text.as_deref().unwrap_or("");
    assert!(
        insert.starts_with("\\Exception"),
        "Stub conflict should insert FQN with leading backslash, got: {:?}",
        insert
    );
}

/// When there is no conflict, auto-import should still work normally.
#[tokio::test]
async fn test_no_conflict_auto_import_still_works() {
    let backend = create_test_backend_with_stubs();

    {
        let mut idx = backend.fqn_uri_index().write();
        idx.insert(
            "App\\Services\\PaymentService".to_string(),
            "file:///app/Services/PaymentService.php".to_string(),
        );
    }

    let uri = Url::parse("file:///controller.php").unwrap();
    let text = concat!(
        "<?php\n",
        "namespace App\\Controllers;\n",
        "use Cassandra\\Exception;\n",
        "\n",
        "new Payment\n",
    );

    let items = complete_at(&backend, &uri, text, 4, 11).await;
    let payment = items
        .iter()
        .find(|i| i.detail.as_deref() == Some("App\\Services\\PaymentService"))
        .expect("Should have PaymentService completion");

    // No conflict — should still have a normal use statement.
    let edits = payment
        .additional_text_edits
        .as_ref()
        .expect("Non-conflicting import should have additional_text_edits");

    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].new_text, "use App\\Services\\PaymentService;\n",);
}

/// Conflict resolution with `new` keyword should produce `\FQN()` snippet.
#[tokio::test]
async fn test_conflicting_use_import_with_new_keyword_inserts_fqn_snippet() {
    let backend = create_test_backend_with_stubs();

    {
        let mut idx = backend.fqn_uri_index().write();
        idx.insert(
            "App\\Exception".to_string(),
            "file:///app/Exception.php".to_string(),
        );
    }

    let uri = Url::parse("file:///controller.php").unwrap();
    let text = concat!(
        "<?php\n",
        "namespace App\\Controllers;\n",
        "use Cassandra\\Exception;\n",
        "\n",
        "new Exc\n",
    );

    let items = complete_at(&backend, &uri, text, 4, 7).await;
    let app_exc = items
        .iter()
        .find(|i| i.detail.as_deref() == Some("App\\Exception"))
        .expect("Should have App\\Exception completion");

    let insert = app_exc.insert_text.as_deref().unwrap_or("");
    // In new context, the insert text should include parentheses.
    assert!(
        insert.starts_with("\\App\\Exception("),
        "new + conflict should insert \\FQN() snippet, got: {:?}",
        insert
    );
    assert!(
        app_exc.additional_text_edits.is_none(),
        "Conflict with new keyword should not produce a use statement"
    );
}

/// When the same FQN is already imported (exact match), it should NOT
/// be treated as a conflict.  This verifies the function itself — in
/// practice the dedup logic in source 1 would prevent this from
/// reaching sources 3-5.
#[tokio::test]
async fn test_same_fqn_already_imported_is_not_a_conflict() {
    let backend = create_test_backend_with_stubs();

    // Put the class in class_index with a FQN that matches the import.
    {
        let mut idx = backend.fqn_uri_index().write();
        idx.insert(
            "Cassandra\\Exception".to_string(),
            "file:///vendor/cassandra/Exception.php".to_string(),
        );
    }

    let uri = Url::parse("file:///controller.php").unwrap();
    let text = concat!(
        "<?php\n",
        "namespace App\\Controllers;\n",
        "use Cassandra\\Exception;\n",
        "\n",
        "Exc\n",
    );

    let items = complete_at(&backend, &uri, text, 4, 3).await;
    // The use-imported entry (source 1) should appear with the short
    // name — no FQN fallback needed.
    let exc = items
        .iter()
        .find(|i| i.detail.as_deref() == Some("Cassandra\\Exception"))
        .expect("Should have Cassandra\\Exception completion");

    let insert = exc.insert_text.as_deref().unwrap_or("");
    assert!(
        !insert.starts_with('\\'),
        "Same-FQN import should use short name, got: {:?}",
        insert
    );
}

/// Multiple conflicting classes: both App\Exception and Domain\Exception
/// should each fall back to FQN when Cassandra\Exception is imported.
#[tokio::test]
async fn test_multiple_conflicting_classes_all_use_fqn() {
    let backend = create_test_backend_with_stubs();

    {
        let mut idx = backend.fqn_uri_index().write();
        idx.insert(
            "App\\Exception".to_string(),
            "file:///app/Exception.php".to_string(),
        );
        idx.insert(
            "Domain\\Exception".to_string(),
            "file:///domain/Exception.php".to_string(),
        );
    }

    let uri = Url::parse("file:///controller.php").unwrap();
    let text = concat!(
        "<?php\n",
        "namespace App\\Controllers;\n",
        "use Cassandra\\Exception;\n",
        "\n",
        "Exc\n",
    );

    let items = complete_at(&backend, &uri, text, 4, 3).await;

    for fqn in &["App\\Exception", "Domain\\Exception"] {
        let item = items
            .iter()
            .find(|i| i.detail.as_deref() == Some(*fqn))
            .unwrap_or_else(|| panic!("Should have {} completion", fqn));

        assert!(
            item.additional_text_edits.is_none(),
            "{} should not produce a use statement",
            fqn
        );

        let insert = item.insert_text.as_deref().unwrap_or("");
        assert!(
            insert.starts_with(&format!("\\{}", fqn)),
            "{} should insert FQN with leading backslash, got: {:?}",
            fqn,
            insert
        );
    }
}

// ─── FQN-mode leading-segment alias collision ──────────────────────────────

/// When the user types a namespace-qualified name like `pq\Exc` (FQN mode)
/// and an existing alias matches the first segment (`use Exception as pq;`),
/// the insert text must get a leading `\` so PHP resolves from the global
/// namespace instead of through the alias.
#[tokio::test]
async fn test_fqn_mode_leading_segment_alias_collision() {
    let mut stubs: HashMap<&'static str, &'static str> = HashMap::new();
    stubs.insert(
        "pq\\Exception",
        "<?php\nnamespace pq;\nclass Exception extends \\Exception {}\n",
    );
    let backend = Backend::new_test_with_stubs(stubs);

    let uri = Url::parse("file:///controller.php").unwrap();
    // `use Exception as pq;` — the alias `pq` collides with the
    // first segment of `pq\Exception`.
    let text = concat!(
        "<?php\n",
        "namespace Demo;\n",
        "use Exception as pq;\n",
        "\n",
        "throw new pq\\Exc\n",
    );

    let items = complete_at(&backend, &uri, text, 4, 17).await;
    let pq_exc = items
        .iter()
        .find(|i| i.detail.as_deref() == Some("pq\\Exception"))
        .expect("Should have pq\\Exception completion");

    let insert = pq_exc.insert_text.as_deref().unwrap_or("");
    assert!(
        insert.starts_with("\\pq\\Exception"),
        "FQN mode with alias collision should prepend \\, got: {:?}",
        insert
    );

    // The text_edit range must cover the full typed prefix (`pq\Exc`,
    // 6 chars) so the editor replaces it entirely with `\pq\Exception`.
    // Without this, the editor only replaces `Exc` and you get
    // `pq\\pq\Exception`.
    let edit = pq_exc
        .text_edit
        .as_ref()
        .expect("FQN mode should produce a text_edit");
    match edit {
        CompletionTextEdit::Edit(te) => {
            let replaced_len = te.range.end.character - te.range.start.character;
            assert_eq!(
                replaced_len,
                "pq\\Exc".len() as u32,
                "text_edit range should cover the full pq\\Exc prefix, got range {:?}",
                te.range
            );
            assert!(
                te.new_text.starts_with("\\pq\\Exception"),
                "text_edit new_text should start with \\pq\\Exception, got: {:?}",
                te.new_text
            );
        }
        _ => panic!("Expected CompletionTextEdit::Edit"),
    }
}

/// When the alias does NOT collide with the first segment, the FQN is
/// inserted as-is (no leading `\` added).
#[tokio::test]
async fn test_fqn_mode_no_alias_collision_keeps_bare_fqn() {
    let mut stubs: HashMap<&'static str, &'static str> = HashMap::new();
    stubs.insert(
        "pq\\Exception",
        "<?php\nnamespace pq;\nclass Exception extends \\Exception {}\n",
    );
    let backend = Backend::new_test_with_stubs(stubs);

    let uri = Url::parse("file:///controller.php").unwrap();
    // `use Exception;` — alias is `Exception`, not `pq`, so no
    // leading-segment collision.
    let text = concat!(
        "<?php\n",
        "namespace Demo;\n",
        "use Exception;\n",
        "\n",
        "throw new pq\\Exc\n",
    );

    let items = complete_at(&backend, &uri, text, 4, 17).await;
    let pq_exc = items
        .iter()
        .find(|i| i.detail.as_deref() == Some("pq\\Exception"))
        .expect("Should have pq\\Exception completion");

    let insert = pq_exc.insert_text.as_deref().unwrap_or("");
    // No leading-segment collision, but there IS a short-name collision
    // (alias `Exception` vs short name `Exception` of `pq\Exception`).
    // In FQN mode, use_import is None so the short-name conflict check
    // does not apply. The insert text stays as the bare FQN.
    assert!(
        !insert.starts_with('\\'),
        "No alias collision should keep bare FQN, got: {:?}",
        insert
    );
}

/// FQN mode with leading `\` typed by the user: the insert text already
/// has `\`, so no extra prefixing is needed regardless of aliases.
#[tokio::test]
async fn test_fqn_mode_user_typed_leading_backslash_unaffected() {
    let mut stubs: HashMap<&'static str, &'static str> = HashMap::new();
    stubs.insert(
        "pq\\Exception",
        "<?php\nnamespace pq;\nclass Exception extends \\Exception {}\n",
    );
    let backend = Backend::new_test_with_stubs(stubs);

    let uri = Url::parse("file:///controller.php").unwrap();
    let text = concat!(
        "<?php\n",
        "namespace Demo;\n",
        "use Exception as pq;\n",
        "\n",
        "throw new \\pq\\Exc\n",
    );

    let items = complete_at(&backend, &uri, text, 4, 18).await;
    let pq_exc = items
        .iter()
        .find(|i| i.detail.as_deref() == Some("pq\\Exception"))
        .expect("Should have pq\\Exception completion");

    let insert = pq_exc.insert_text.as_deref().unwrap_or("");
    assert!(
        insert.starts_with("\\pq\\Exception"),
        "User-typed leading \\ should be preserved, got: {:?}",
        insert
    );
    // Must not double the backslash.
    assert!(
        !insert.starts_with("\\\\"),
        "Should not double the leading \\, got: {:?}",
        insert
    );
}

// ─── Namespace segment completion ───────────────────────────────────────

/// Helper: extract MODULE-kind items from a completion list.
fn module_items(items: &[CompletionItem]) -> Vec<&CompletionItem> {
    items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::MODULE))
        .collect()
}

#[tokio::test]
async fn test_namespace_segments_in_use_import() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": {
                    "App\\": "src/"
                }
            }
        }"#,
        &[
            (
                "src/Models/User.php",
                concat!("<?php\n", "namespace App\\Models;\n", "class User {}\n",),
            ),
            (
                "src/Models/Post.php",
                concat!("<?php\n", "namespace App\\Models;\n", "class Post {}\n",),
            ),
            (
                "src/Services/AuthService.php",
                concat!(
                    "<?php\n",
                    "namespace App\\Services;\n",
                    "class AuthService {}\n",
                ),
            ),
        ],
    );

    // Open files so they're in ast_map.
    for relpath in &[
        "src/Models/User.php",
        "src/Models/Post.php",
        "src/Services/AuthService.php",
    ] {
        let file_uri =
            Url::parse(&format!("file://{}", _dir.path().join(relpath).display())).unwrap();
        let content = fs::read_to_string(_dir.path().join(relpath)).unwrap();
        backend
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: file_uri,
                    language_id: "php".to_string(),
                    version: 1,
                    text: content,
                },
            })
            .await;
    }

    let uri = Url::parse("file:///use_ns_segment.php").unwrap();
    // Typing `use App\` — cursor at column 8.
    let text = concat!("<?php\n", "use App\\\n",);

    let items = complete_at(&backend, &uri, text, 1, 8).await;
    let modules = module_items(&items);
    let module_labels: Vec<&str> = modules.iter().map(|i| i.label.as_str()).collect();

    assert!(
        module_labels.contains(&"App\\Models"),
        "Should suggest App\\Models namespace segment, got: {:?}",
        module_labels
    );
    assert!(
        module_labels.contains(&"App\\Services"),
        "Should suggest App\\Services namespace segment, got: {:?}",
        module_labels
    );

    // Segments should have MODULE kind.
    for item in &modules {
        assert_eq!(item.kind, Some(CompletionItemKind::MODULE));
    }

    // Segments should have detail like "namespace App\Models".
    let models_item = modules.iter().find(|i| i.label == "App\\Models").unwrap();
    assert_eq!(models_item.detail.as_deref(), Some("namespace App\\Models"),);

    // Classes should also still be present.
    let classes = class_items(&items);
    assert!(
        classes
            .iter()
            .any(|i| i.detail.as_deref() == Some("App\\Models\\User")),
        "Classes should still appear alongside namespace segments"
    );
}

#[tokio::test]
async fn test_namespace_segments_sort_above_classes() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": {
                    "App\\": "src/"
                }
            }
        }"#,
        &[
            (
                "src/Models/User.php",
                concat!("<?php\n", "namespace App\\Models;\n", "class User {}\n",),
            ),
            (
                "src/Helper.php",
                concat!("<?php\n", "namespace App;\n", "class Helper {}\n",),
            ),
        ],
    );

    for relpath in &["src/Models/User.php", "src/Helper.php"] {
        let file_uri =
            Url::parse(&format!("file://{}", _dir.path().join(relpath).display())).unwrap();
        let content = fs::read_to_string(_dir.path().join(relpath)).unwrap();
        backend
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: file_uri,
                    language_id: "php".to_string(),
                    version: 1,
                    text: content,
                },
            })
            .await;
    }

    let uri = Url::parse("file:///sort_test.php").unwrap();
    let text = concat!("<?php\n", "use App\\\n",);
    let items = complete_at(&backend, &uri, text, 1, 8).await;

    let models_segment = items
        .iter()
        .find(|i| i.label == "App\\Models" && i.kind == Some(CompletionItemKind::MODULE))
        .expect("Should have App\\Models segment");

    let helper_class = items
        .iter()
        .find(|i| i.detail.as_deref() == Some("App\\Helper"))
        .expect("Should have App\\Helper class");

    // Namespace segments sort before class items.
    assert!(
        models_segment.sort_text < helper_class.sort_text,
        "Namespace segment sort_text ({:?}) should be before class sort_text ({:?})",
        models_segment.sort_text,
        helper_class.sort_text
    );
}

#[tokio::test]
async fn test_namespace_segments_no_semicolon_in_use_context() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": {
                    "App\\": "src/"
                }
            }
        }"#,
        &[(
            "src/Models/User.php",
            concat!("<?php\n", "namespace App\\Models;\n", "class User {}\n",),
        )],
    );

    let user_uri = Url::parse(&format!(
        "file://{}",
        _dir.path().join("src/Models/User.php").display()
    ))
    .unwrap();
    let user_content = fs::read_to_string(_dir.path().join("src/Models/User.php")).unwrap();
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: user_uri,
                language_id: "php".to_string(),
                version: 1,
                text: user_content,
            },
        })
        .await;

    let uri = Url::parse("file:///use_ns_semi.php").unwrap();
    let text = concat!("<?php\n", "use App\\\n",);
    let items = complete_at(&backend, &uri, text, 1, 8).await;

    let modules = module_items(&items);
    assert!(
        !modules.is_empty(),
        "Should have at least one namespace segment"
    );

    for item in &modules {
        let insert = item.insert_text.as_deref().unwrap_or(&item.label);
        assert!(
            !insert.ends_with(';'),
            "Namespace segments in use context should NOT end with ';', got: {:?}",
            insert
        );
    }

    // Classes in use context should still have semicolons.
    let classes = class_items(&items);
    for item in &classes {
        let insert = item.insert_text.as_deref().unwrap_or(&item.label);
        assert!(
            insert.ends_with(';'),
            "Classes in use context should end with ';', got: {:?}",
            insert
        );
    }
}

#[tokio::test]
async fn test_namespace_segments_with_leading_backslash() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": {
                    "App\\": "src/"
                }
            }
        }"#,
        &[
            (
                "src/Models/User.php",
                concat!("<?php\n", "namespace App\\Models;\n", "class User {}\n",),
            ),
            (
                "src/Services/Auth.php",
                concat!("<?php\n", "namespace App\\Services;\n", "class Auth {}\n",),
            ),
        ],
    );

    for relpath in &["src/Models/User.php", "src/Services/Auth.php"] {
        let file_uri =
            Url::parse(&format!("file://{}", _dir.path().join(relpath).display())).unwrap();
        let content = fs::read_to_string(_dir.path().join(relpath)).unwrap();
        backend
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: file_uri,
                    language_id: "php".to_string(),
                    version: 1,
                    text: content,
                },
            })
            .await;
    }

    let uri = Url::parse("file:///leading_bs.php").unwrap();
    // `\App\` — cursor at column 9 (`new \App\`).
    let text = concat!("<?php\n", "new \\App\\\n",);
    let items = complete_at(&backend, &uri, text, 1, 9).await;

    let modules = module_items(&items);
    let module_labels: Vec<&str> = modules.iter().map(|i| i.label.as_str()).collect();

    assert!(
        module_labels.contains(&"App\\Models"),
        "Should suggest App\\Models with leading backslash, got: {:?}",
        module_labels
    );

    // Insert text should include leading `\`.
    let models_item = modules.iter().find(|i| i.label == "App\\Models").unwrap();
    let insert = models_item.insert_text.as_deref().unwrap();
    assert_eq!(
        insert, "\\App\\Models",
        "Insert text should include leading backslash"
    );
}

#[tokio::test]
async fn test_namespace_segments_in_type_hint() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": {
                    "App\\": "src/"
                }
            }
        }"#,
        &[(
            "src/Models/User.php",
            concat!("<?php\n", "namespace App\\Models;\n", "class User {}\n",),
        )],
    );

    let user_uri = Url::parse(&format!(
        "file://{}",
        _dir.path().join("src/Models/User.php").display()
    ))
    .unwrap();
    let user_content = fs::read_to_string(_dir.path().join("src/Models/User.php")).unwrap();
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: user_uri,
                language_id: "php".to_string(),
                version: 1,
                text: user_content,
            },
        })
        .await;

    let uri = Url::parse("file:///typehint_ns.php").unwrap();
    // Type hint context: `function foo(\App\`
    let text = concat!("<?php\n", "function foo(\\App\\\n",);
    let items = complete_at(&backend, &uri, text, 1, 18).await;

    let modules = module_items(&items);
    let module_labels: Vec<&str> = modules.iter().map(|i| i.label.as_str()).collect();

    assert!(
        module_labels.contains(&"App\\Models"),
        "Should suggest namespace segments in type hint context, got: {:?}",
        module_labels
    );
}

#[tokio::test]
async fn test_namespace_segments_filtered_by_partial() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": {
                    "App\\": "src/"
                }
            }
        }"#,
        &[
            (
                "src/Models/User.php",
                concat!("<?php\n", "namespace App\\Models;\n", "class User {}\n",),
            ),
            (
                "src/Services/Auth.php",
                concat!("<?php\n", "namespace App\\Services;\n", "class Auth {}\n",),
            ),
        ],
    );

    for relpath in &["src/Models/User.php", "src/Services/Auth.php"] {
        let file_uri =
            Url::parse(&format!("file://{}", _dir.path().join(relpath).display())).unwrap();
        let content = fs::read_to_string(_dir.path().join(relpath)).unwrap();
        backend
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: file_uri,
                    language_id: "php".to_string(),
                    version: 1,
                    text: content,
                },
            })
            .await;
    }

    let uri = Url::parse("file:///filter_ns.php").unwrap();
    // `use App\M` — partial "M" should filter to Models, not Services.
    let text = concat!("<?php\n", "use App\\M\n",);
    let items = complete_at(&backend, &uri, text, 1, 9).await;

    let modules = module_items(&items);
    let module_labels: Vec<&str> = modules.iter().map(|i| i.label.as_str()).collect();

    assert!(
        module_labels.contains(&"App\\Models"),
        "App\\Models should match partial 'M', got: {:?}",
        module_labels
    );
    assert!(
        !module_labels.contains(&"App\\Services"),
        "App\\Services should NOT match partial 'M', got: {:?}",
        module_labels
    );
}

#[tokio::test]
async fn test_namespace_segments_deep_nesting() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": {
                    "App\\": "src/"
                }
            }
        }"#,
        &[
            (
                "src/Http/Controllers/Admin/UserController.php",
                concat!(
                    "<?php\n",
                    "namespace App\\Http\\Controllers\\Admin;\n",
                    "class UserController {}\n",
                ),
            ),
            (
                "src/Http/Controllers/Api/AuthController.php",
                concat!(
                    "<?php\n",
                    "namespace App\\Http\\Controllers\\Api;\n",
                    "class AuthController {}\n",
                ),
            ),
            (
                "src/Http/Middleware/AuthMiddleware.php",
                concat!(
                    "<?php\n",
                    "namespace App\\Http\\Middleware;\n",
                    "class AuthMiddleware {}\n",
                ),
            ),
        ],
    );

    for relpath in &[
        "src/Http/Controllers/Admin/UserController.php",
        "src/Http/Controllers/Api/AuthController.php",
        "src/Http/Middleware/AuthMiddleware.php",
    ] {
        let file_uri =
            Url::parse(&format!("file://{}", _dir.path().join(relpath).display())).unwrap();
        let content = fs::read_to_string(_dir.path().join(relpath)).unwrap();
        backend
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: file_uri,
                    language_id: "php".to_string(),
                    version: 1,
                    text: content,
                },
            })
            .await;
    }

    // First level: `use App\` should show `App\Http`.
    let uri = Url::parse("file:///deep_ns1.php").unwrap();
    let text = concat!("<?php\n", "use App\\\n",);
    let items = complete_at(&backend, &uri, text, 1, 8).await;
    let modules = module_items(&items);
    let module_labels: Vec<&str> = modules.iter().map(|i| i.label.as_str()).collect();
    assert!(
        module_labels.contains(&"App\\Http"),
        "First level should show App\\Http, got: {:?}",
        module_labels
    );

    // Second level: `use App\Http\` should show Controllers and Middleware.
    let uri2 = Url::parse("file:///deep_ns2.php").unwrap();
    let text2 = concat!("<?php\n", "use App\\Http\\\n",);
    let items2 = complete_at(&backend, &uri2, text2, 1, 13).await;
    let modules2 = module_items(&items2);
    let module_labels2: Vec<&str> = modules2.iter().map(|i| i.label.as_str()).collect();
    assert!(
        module_labels2.contains(&"App\\Http\\Controllers"),
        "Second level should show App\\Http\\Controllers, got: {:?}",
        module_labels2
    );
    assert!(
        module_labels2.contains(&"App\\Http\\Middleware"),
        "Second level should show App\\Http\\Middleware, got: {:?}",
        module_labels2
    );

    // Third level: `use App\Http\Controllers\` should show Admin and Api.
    let uri3 = Url::parse("file:///deep_ns3.php").unwrap();
    let text3 = concat!("<?php\n", "use App\\Http\\Controllers\\\n",);
    let items3 = complete_at(&backend, &uri3, text3, 1, 25).await;
    let modules3 = module_items(&items3);
    let module_labels3: Vec<&str> = modules3.iter().map(|i| i.label.as_str()).collect();
    assert!(
        module_labels3.contains(&"App\\Http\\Controllers\\Admin"),
        "Third level should show Admin, got: {:?}",
        module_labels3
    );
    assert!(
        module_labels3.contains(&"App\\Http\\Controllers\\Api"),
        "Third level should show Api, got: {:?}",
        module_labels3
    );
}

#[tokio::test]
async fn test_namespace_segments_same_namespace_simplifies() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": {
                    "App\\": "src/"
                }
            }
        }"#,
        &[(
            "src/Models/User.php",
            concat!("<?php\n", "namespace App\\Models;\n", "class User {}\n",),
        )],
    );

    let user_uri = Url::parse(&format!(
        "file://{}",
        _dir.path().join("src/Models/User.php").display()
    ))
    .unwrap();
    let user_content = fs::read_to_string(_dir.path().join("src/Models/User.php")).unwrap();
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: user_uri,
                language_id: "php".to_string(),
                version: 1,
                text: user_content,
            },
        })
        .await;

    // The cursor file is in the `App` namespace, so `App\Models` should
    // be simplified to just `Models`.
    let uri = Url::parse("file:///same_ns_segment.php").unwrap();
    let text = concat!("<?php\n", "namespace App;\n", "new App\\M\n",);
    let items = complete_at(&backend, &uri, text, 2, 9).await;

    let modules = module_items(&items);
    let models_segment = modules
        .iter()
        .find(|i| i.detail.as_deref() == Some("namespace App\\Models"));

    assert!(
        models_segment.is_some(),
        "Should have App\\Models namespace segment, got modules: {:?}",
        modules
            .iter()
            .map(|i| (&i.label, &i.detail))
            .collect::<Vec<_>>()
    );

    let seg = models_segment.unwrap();
    assert_eq!(
        seg.label, "Models",
        "Label should be simplified to relative name within same namespace"
    );
    assert_eq!(
        seg.insert_text.as_deref(),
        Some("Models"),
        "Insert text should be the relative name"
    );
}

#[tokio::test]
async fn test_namespace_segments_from_stubs() {
    let mut stubs: HashMap<&'static str, &'static str> = HashMap::new();
    stubs.insert("Ds\\Map", "<?php\nnamespace Ds;\nclass Map {}\n");
    stubs.insert("Ds\\Set", "<?php\nnamespace Ds;\nclass Set {}\n");
    let backend = Backend::new_test_with_stubs(stubs);

    let uri = Url::parse("file:///stub_ns.php").unwrap();
    let text = concat!("<?php\n", "use Ds\\\n",);
    let items = complete_at(&backend, &uri, text, 1, 7).await;

    // Should have Ds\Map and Ds\Set as classes.
    let classes = class_items(&items);
    assert!(
        classes
            .iter()
            .any(|i| i.detail.as_deref() == Some("Ds\\Map")),
        "Should have Ds\\Map class"
    );
    assert!(
        classes
            .iter()
            .any(|i| i.detail.as_deref() == Some("Ds\\Set")),
        "Should have Ds\\Set class"
    );

    // No namespace segments — Ds\Map and Ds\Set are leaf classes
    // with no deeper nesting.
    let modules = module_items(&items);
    assert!(
        modules.is_empty(),
        "Should have no namespace segments for flat namespace, got: {:?}",
        modules.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn test_namespace_segments_from_stubs_with_nesting() {
    let mut stubs: HashMap<&'static str, &'static str> = HashMap::new();
    stubs.insert(
        "Cassandra\\Exception\\AlreadyExistsException",
        "<?php\nnamespace Cassandra\\Exception;\nclass AlreadyExistsException {}\n",
    );
    stubs.insert(
        "Cassandra\\Cluster",
        "<?php\nnamespace Cassandra;\nclass Cluster {}\n",
    );
    let backend = Backend::new_test_with_stubs(stubs);

    let uri = Url::parse("file:///stub_nested_ns.php").unwrap();
    let text = concat!("<?php\n", "use Cassandra\\\n",);
    let items = complete_at(&backend, &uri, text, 1, 14).await;

    let modules = module_items(&items);
    let module_labels: Vec<&str> = modules.iter().map(|i| i.label.as_str()).collect();

    assert!(
        module_labels.contains(&"Cassandra\\Exception"),
        "Should suggest Cassandra\\Exception namespace segment, got: {:?}",
        module_labels
    );
}

#[tokio::test]
async fn test_namespace_segments_text_edit_replaces_full_prefix() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": {
                    "App\\": "src/"
                }
            }
        }"#,
        &[(
            "src/Models/User.php",
            concat!("<?php\n", "namespace App\\Models;\n", "class User {}\n",),
        )],
    );

    let user_uri = Url::parse(&format!(
        "file://{}",
        _dir.path().join("src/Models/User.php").display()
    ))
    .unwrap();
    let user_content = fs::read_to_string(_dir.path().join("src/Models/User.php")).unwrap();
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: user_uri,
                language_id: "php".to_string(),
                version: 1,
                text: user_content,
            },
        })
        .await;

    let uri = Url::parse("file:///textedit_ns.php").unwrap();
    // `use App\M` — 9 characters on line 1
    let text = concat!("<?php\n", "use App\\M\n",);
    let items = complete_at(&backend, &uri, text, 1, 9).await;

    let modules = module_items(&items);
    let models_item = modules
        .iter()
        .find(|i| i.label == "App\\Models")
        .expect("Should have App\\Models segment");

    // The text_edit should replace the full `App\M` prefix.
    if let Some(CompletionTextEdit::Edit(ref edit)) = models_item.text_edit {
        assert_eq!(edit.range.start.line, 1);
        assert_eq!(
            edit.range.start.character, 4,
            "text_edit should start at the beginning of 'App\\M' (after 'use ')"
        );
        assert_eq!(edit.range.end.line, 1);
        assert_eq!(edit.range.end.character, 9);
        assert_eq!(edit.new_text, "App\\Models");
    } else {
        panic!(
            "Namespace segment should have a text_edit, got: {:?}",
            models_item.text_edit
        );
    }
}

#[tokio::test]
async fn test_namespace_segments_not_injected_for_bare_name() {
    let backend = create_test_backend_with_stubs();

    let uri = Url::parse("file:///bare_name.php").unwrap();
    // Typing `use DateT` — no backslash in the typed prefix.
    // Even though UseImport forces is_fqn_prefix, no segments
    // should appear because there's no namespace to browse.
    let text = concat!("<?php\n", "use DateT\n",);
    let items = complete_at(&backend, &uri, text, 1, 9).await;

    let modules = module_items(&items);
    assert!(
        modules.is_empty(),
        "Bare name without backslash should not produce namespace segments, got: {:?}",
        modules.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn test_namespace_segments_in_new_context() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": {
                    "App\\": "src/"
                }
            }
        }"#,
        &[(
            "src/Models/User.php",
            concat!("<?php\n", "namespace App\\Models;\n", "class User {}\n",),
        )],
    );

    let user_uri = Url::parse(&format!(
        "file://{}",
        _dir.path().join("src/Models/User.php").display()
    ))
    .unwrap();
    let user_content = fs::read_to_string(_dir.path().join("src/Models/User.php")).unwrap();
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: user_uri,
                language_id: "php".to_string(),
                version: 1,
                text: user_content,
            },
        })
        .await;

    let uri = Url::parse("file:///new_ns.php").unwrap();
    let text = concat!("<?php\n", "new App\\\n",);
    let items = complete_at(&backend, &uri, text, 1, 8).await;

    let modules = module_items(&items);
    let module_labels: Vec<&str> = modules.iter().map(|i| i.label.as_str()).collect();

    assert!(
        module_labels.contains(&"App\\Models"),
        "Namespace segments should appear in `new` context, got: {:?}",
        module_labels
    );

    // Segments should NOT have snippet format (no parentheses).
    let models_item = modules.iter().find(|i| i.label == "App\\Models").unwrap();
    assert!(
        models_item.insert_text_format.is_none()
            || models_item.insert_text_format == Some(InsertTextFormat::PLAIN_TEXT),
        "Namespace segments should not have snippet format"
    );
}

#[tokio::test]
async fn test_namespace_segments_deduplicated() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": {
                    "App\\": "src/"
                }
            }
        }"#,
        &[
            (
                "src/Models/User.php",
                concat!("<?php\n", "namespace App\\Models;\n", "class User {}\n",),
            ),
            (
                "src/Models/Post.php",
                concat!("<?php\n", "namespace App\\Models;\n", "class Post {}\n",),
            ),
            (
                "src/Models/Comment.php",
                concat!("<?php\n", "namespace App\\Models;\n", "class Comment {}\n",),
            ),
        ],
    );

    for relpath in &[
        "src/Models/User.php",
        "src/Models/Post.php",
        "src/Models/Comment.php",
    ] {
        let file_uri =
            Url::parse(&format!("file://{}", _dir.path().join(relpath).display())).unwrap();
        let content = fs::read_to_string(_dir.path().join(relpath)).unwrap();
        backend
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: file_uri,
                    language_id: "php".to_string(),
                    version: 1,
                    text: content,
                },
            })
            .await;
    }

    let uri = Url::parse("file:///dedup_ns.php").unwrap();
    let text = concat!("<?php\n", "use App\\\n",);
    let items = complete_at(&backend, &uri, text, 1, 8).await;

    let modules = module_items(&items);
    let models_count = modules.iter().filter(|i| i.label == "App\\Models").count();

    assert_eq!(
        models_count, 1,
        "App\\Models should appear exactly once, got {} occurrences",
        models_count
    );
}

// ─── label is FQN tests ────────────────────────────────────────────────────

/// In a method body (non-FQN mode), the label for a namespaced class
/// should be the fully-qualified name.
#[tokio::test]
async fn test_class_name_completion_label_is_short_name_in_method_body() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": {
                    "App\\": "src/"
                }
            }
        }"#,
        &[(
            "src/Models/Order.php",
            concat!(
                "<?php\n",
                "namespace App\\Models;\n",
                "class Order {\n",
                "    public function id(): int { return 1; }\n",
                "}\n",
            ),
        )],
    );

    // Open the Order file so it's in ast_map
    let order_uri = Url::parse(&format!(
        "file://{}",
        _dir.path().join("src/Models/Order.php").display()
    ))
    .unwrap();
    let order_content = std::fs::read_to_string(_dir.path().join("src/Models/Order.php")).unwrap();
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: order_uri,
                language_id: "php".to_string(),
                version: 1,
                text: order_content,
            },
        })
        .await;

    let uri = Url::parse("file:///test_label_details.php").unwrap();
    let text = concat!(
        "<?php\n",
        "namespace App\\Http\\Controllers;\n",
        "class TestController {\n",
        "    public function index() {\n",
        "        $order = new Orde\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 4, 25).await;
    let classes = class_items(&items);
    let order_item = find_by_fqn(&classes, "App\\Models\\Order");

    assert!(
        order_item.is_some(),
        "Should find 'App\\Models\\Order' in completions (via detail), got: {:?}",
        fqns(&classes)
    );
    // In non-FQN mode the label is the short name
    assert_eq!(
        order_item.unwrap().label,
        "Order",
        "Label should be the short name in non-FQN mode"
    );
}

/// In a `use` import context, the label should also be the FQN.
#[tokio::test]
async fn test_class_name_completion_label_is_fqn_in_use_import() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": {
                    "App\\": "src/"
                }
            }
        }"#,
        &[(
            "src/Models/Order.php",
            concat!("<?php\n", "namespace App\\Models;\n", "class Order {}\n",),
        )],
    );

    // Open the Order file so it's in ast_map
    let order_uri = Url::parse(&format!(
        "file://{}",
        _dir.path().join("src/Models/Order.php").display()
    ))
    .unwrap();
    let order_content = std::fs::read_to_string(_dir.path().join("src/Models/Order.php")).unwrap();
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: order_uri,
                language_id: "php".to_string(),
                version: 1,
                text: order_content,
            },
        })
        .await;

    let uri = Url::parse("file:///test_use_ld.php").unwrap();
    let text = concat!("<?php\n", "use App\\Models\\Orde\n",);

    let items = complete_at(&backend, &uri, text, 1, 19).await;
    let classes = class_items(&items);
    let order_item = classes.iter().find(|i| i.label == "App\\Models\\Order");

    assert!(
        order_item.is_some(),
        "Should find 'App\\Models\\Order' in use-import completions, got: {:?}",
        classes.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
}

/// Global classes (no namespace) should have label == short name (FQN == short name).
#[tokio::test]
async fn test_class_name_completion_label_is_short_name_for_global() {
    let backend = create_test_backend_with_stubs();

    let uri = Url::parse("file:///test_global_ld.php").unwrap();
    let text = concat!("<?php\n", "class MyLocalClass {}\n", "new MyLocal\n",);

    let items = complete_at(&backend, &uri, text, 2, 11).await;
    let classes = class_items(&items);
    let local = classes.iter().find(|i| i.label == "MyLocalClass");

    assert!(
        local.is_some(),
        "Should find 'MyLocalClass' (FQN == short name for globals), got: {:?}",
        classes.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
}

/// Classes from the classmap source should have short name as label
/// and FQN in the detail field when prefix has no namespace separator.
#[tokio::test]
async fn test_class_name_completion_classmap_label_is_short_name() {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    fs::write(
        dir.path().join("composer.json"),
        r#"{"name": "test/project"}"#,
    )
    .expect("failed to write composer.json");

    let composer_dir = dir.path().join("vendor").join("composer");
    fs::create_dir_all(&composer_dir).expect("failed to create vendor/composer");
    fs::write(
        composer_dir.join("autoload_classmap.php"),
        concat!(
            "<?php\n",
            "$vendorDir = dirname(__DIR__);\n",
            "$baseDir = dirname($vendorDir);\n",
            "return array(\n",
            "    'Vendor\\\\Payments\\\\Invoice' => $vendorDir . '/pkg/src/Invoice.php',\n",
            ");\n",
        ),
    )
    .expect("failed to write autoload_classmap.php");

    let backend = Backend::new_test_with_workspace(dir.path().to_path_buf(), vec![]);
    let classmap = parse_autoload_classmap(dir.path(), "vendor");
    {
        let mut idx = backend.fqn_uri_index().write();
        for (fqn, path) in &classmap {
            idx.insert(fqn.clone(), Url::from_file_path(path).unwrap().to_string());
        }
    }

    let uri = Url::parse("file:///test_cm_ld.php").unwrap();
    let text = concat!("<?php\n", "new Invoic\n",);

    let items = complete_at(&backend, &uri, text, 1, 10).await;
    let classes = class_items(&items);
    let invoice = find_by_fqn(&classes, "Vendor\\Payments\\Invoice");

    assert!(
        invoice.is_some(),
        "Should find 'Vendor\\Payments\\Invoice' from classmap (via detail), got: {:?}",
        fqns(&classes)
    );
    assert_eq!(
        invoice.unwrap().label,
        "Invoice",
        "Label should be the short name in non-FQN mode"
    );
}

/// Classes from the class_index source should have short name as label
/// and FQN in the detail field when prefix has no namespace separator.
#[tokio::test]
async fn test_class_name_completion_class_index_label_is_short_name() {
    let backend = create_test_backend_with_stubs();

    {
        let mut idx = backend.fqn_uri_index().write();
        idx.insert(
            "Acme\\Billing\\Receipt".to_string(),
            "file:///acme/Billing/Receipt.php".to_string(),
        );
    }

    let uri = Url::parse("file:///test_idx_ld.php").unwrap();
    let text = concat!("<?php\n", "new Receip\n",);

    let items = complete_at(&backend, &uri, text, 1, 10).await;
    let classes = class_items(&items);
    let receipt = find_by_fqn(&classes, "Acme\\Billing\\Receipt");

    assert!(
        receipt.is_some(),
        "Should find 'Acme\\Billing\\Receipt' from class_index (via detail), got: {:?}",
        fqns(&classes)
    );
    assert_eq!(
        receipt.unwrap().label,
        "Receipt",
        "Label should be the short name in non-FQN mode"
    );
}

// ─── Namespace alias prefix completion ─────────────────────────────────────

/// When the user types `OA\Re` and the file has `use OpenApi\Attributes as OA`,
/// classes under `OpenApi\Attributes` whose short name starts with `Re` should
/// appear in completions (e.g. `OpenApi\Attributes\Response`).
#[tokio::test]
async fn test_namespace_alias_prefix_matches_classes_underneath() {
    let backend = create_test_backend_with_stubs();

    {
        let mut idx = backend.fqn_uri_index().write();
        idx.insert(
            "OpenApi\\Attributes\\Response".to_string(),
            "file:///vendor/openapi/Response.php".to_string(),
        );
        idx.insert(
            "OpenApi\\Attributes\\RequestBody".to_string(),
            "file:///vendor/openapi/RequestBody.php".to_string(),
        );
        idx.insert(
            "OpenApi\\Attributes\\Property".to_string(),
            "file:///vendor/openapi/Property.php".to_string(),
        );
    }

    let uri = Url::parse("file:///controller.php").unwrap();
    let text = concat!(
        "<?php\n",
        "use OpenApi\\Attributes as OA;\n",
        "new OA\\Re\n",
    );

    let items = complete_at(&backend, &uri, text, 2, 9).await;
    let cls = class_items(&items);
    let labels: Vec<&str> = cls.iter().map(|i| i.label.as_str()).collect();

    // Both classes starting with "Re" should appear.
    assert!(
        labels.iter().any(|l| l.contains("Response")),
        "Expected OA\\Response in completions, got: {:?}",
        labels
    );
    assert!(
        labels.iter().any(|l| l.contains("RequestBody")),
        "Expected OA\\RequestBody in completions, got: {:?}",
        labels
    );
    // Property does NOT start with "Re", so it should be absent.
    assert!(
        !labels.iter().any(|l| l.contains("Property")),
        "Property should not match OA\\Re prefix, got: {:?}",
        labels
    );
}

/// The insert text for alias-qualified completions should use the
/// alias form (e.g. `OA\Response`), not the full FQN.
#[tokio::test]
async fn test_namespace_alias_prefix_insert_text_uses_alias() {
    let backend = create_test_backend_with_stubs();

    {
        let mut idx = backend.fqn_uri_index().write();
        idx.insert(
            "OpenApi\\Attributes\\Response".to_string(),
            "file:///vendor/openapi/Response.php".to_string(),
        );
    }

    let uri = Url::parse("file:///controller.php").unwrap();
    let text = concat!(
        "<?php\n",
        "use OpenApi\\Attributes as OA;\n",
        "new OA\\Resp\n",
    );

    let items = complete_at(&backend, &uri, text, 2, 11).await;
    let resp = items
        .iter()
        .find(|i| i.detail.as_deref() == Some("OpenApi\\Attributes\\Response"))
        .expect("Should find OpenApi\\Attributes\\Response");

    let insert = resp.insert_text.as_deref().unwrap_or("");
    assert!(
        insert.contains("OA\\Response"),
        "Insert text should use alias form OA\\Response, got: {:?}",
        insert
    );
}

/// Namespace segments should work through aliases: typing `OA\` should
/// show sub-namespace segments under `OpenApi\Attributes\`.
#[tokio::test]
async fn test_namespace_alias_prefix_shows_segments() {
    let backend = create_test_backend_with_stubs();

    {
        let mut idx = backend.fqn_uri_index().write();
        idx.insert(
            "OpenApi\\Attributes\\Callbacks\\Callback".to_string(),
            "file:///vendor/openapi/Callbacks/Callback.php".to_string(),
        );
        idx.insert(
            "OpenApi\\Attributes\\Response".to_string(),
            "file:///vendor/openapi/Response.php".to_string(),
        );
    }

    let uri = Url::parse("file:///controller.php").unwrap();
    let text = concat!("<?php\n", "use OpenApi\\Attributes as OA;\n", "new OA\\C\n",);

    let items = complete_at(&backend, &uri, text, 2, 8).await;

    // The `Callbacks` sub-namespace segment should appear.
    let callbacks = items.iter().find(|i| i.label.contains("Callbacks"));
    assert!(
        callbacks.is_some(),
        "Expected a Callbacks namespace segment, got labels: {:?}",
        items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
}

/// Typing just `OA\` (alias + backslash, no partial after it) should
/// list all classes under `OpenApi\Attributes`.
#[tokio::test]
async fn test_namespace_alias_prefix_bare_backslash_lists_all() {
    let backend = create_test_backend_with_stubs();

    {
        let mut idx = backend.fqn_uri_index().write();
        idx.insert(
            "OpenApi\\Attributes\\Response".to_string(),
            "file:///vendor/openapi/Response.php".to_string(),
        );
        idx.insert(
            "OpenApi\\Attributes\\Property".to_string(),
            "file:///vendor/openapi/Property.php".to_string(),
        );
    }

    let uri = Url::parse("file:///controller.php").unwrap();
    let text = concat!("<?php\n", "use OpenApi\\Attributes as OA;\n", "new OA\\\n",);

    let items = complete_at(&backend, &uri, text, 2, 7).await;
    let cls = class_items(&items);
    let labels: Vec<&str> = cls.iter().map(|i| i.label.as_str()).collect();

    assert!(
        labels.iter().any(|l| l.contains("Response")),
        "Expected Response in completions for OA\\, got: {:?}",
        labels
    );
    assert!(
        labels.iter().any(|l| l.contains("Property")),
        "Expected Property in completions for OA\\, got: {:?}",
        labels
    );
}

// ─── Namespace completion inferred from file path ───────────────────────────

/// When the file's path matches a PSR-4 mapping, the inferred namespace
/// should appear at the top of the completion list and be preselected.
#[tokio::test]
async fn test_namespace_inferred_from_file_path_basic() {
    let (backend, dir) = create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": {
                    "App\\": "src/"
                }
            }
        }"#,
        &[],
    );

    // Create the directory so the file URI is valid.
    std::fs::create_dir_all(dir.path().join("src/Models")).unwrap();

    let uri = Url::from_file_path(dir.path().join("src/Models/User.php")).unwrap();
    let text = concat!("<?php\n", "namespace \n",);

    let items = complete_at(&backend, &uri, text, 1, 10).await;

    let inferred = items
        .iter()
        .find(|i| i.label == "App\\Models")
        .expect("Should suggest App\\Models inferred from file path");

    assert_eq!(
        inferred.detail.as_deref(),
        Some("(from file path)"),
        "Inferred namespace should have '(from file path)' detail"
    );

    assert_eq!(
        inferred.preselect,
        Some(true),
        "Inferred namespace should be preselected"
    );

    // It should sort before other namespaces.
    let app_only = items.iter().find(|i| i.label == "App").unwrap();
    assert!(
        inferred.sort_text < app_only.sort_text,
        "Inferred namespace should sort before parent namespace: {:?} vs {:?}",
        inferred.sort_text,
        app_only.sort_text
    );
}

/// When multiple PSR-4 roots match the same directory, all inferred
/// namespaces should appear and the most specific one should be first.
#[tokio::test]
async fn test_namespace_inferred_multiple_matches_longest_first() {
    let (backend, dir) = create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": {
                    "Tests\\": "tests/",
                    "Tests\\Support\\": "tests/Support/"
                }
            }
        }"#,
        &[],
    );

    std::fs::create_dir_all(dir.path().join("tests/Support/Helpers")).unwrap();

    let uri = Url::from_file_path(dir.path().join("tests/Support/Helpers/TestHelper.php")).unwrap();
    let text = concat!("<?php\n", "namespace \n",);

    let items = complete_at(&backend, &uri, text, 1, 10).await;

    // Both mappings match, producing the same namespace string but with
    // different specificities.  We should see the namespace present.
    let inferred_items: Vec<&CompletionItem> = items
        .iter()
        .filter(|i| {
            i.label == "Tests\\Support\\Helpers" && i.detail.as_deref() == Some("(from file path)")
        })
        .collect();

    assert!(
        !inferred_items.is_empty(),
        "Should have at least one inferred namespace for Tests\\Support\\Helpers, got labels: {:?}",
        items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );

    // The inferred namespace should be preselected.
    assert_eq!(
        inferred_items[0].preselect,
        Some(true),
        "Most specific inferred namespace should be preselected"
    );
}

/// The real-world Luxplus composer.json scenario: a file in
/// `src/core/Brands/Services/` should infer `Luxplus\Core\Brands\Services`.
#[tokio::test]
async fn test_namespace_inferred_luxplus_real_world() {
    let (backend, dir) = create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": {
                    "Luxplus\\Core\\": "src/core/",
                    "Luxplus\\Core\\Database\\": "src/database/",
                    "Luxplus\\Core\\Tasks\\": "src/tasks/",
                    "Luxplus\\Web\\": "src/web/"
                }
            }
        }"#,
        &[],
    );

    std::fs::create_dir_all(dir.path().join("src/core/Brands/Services")).unwrap();

    let uri = Url::from_file_path(dir.path().join("src/core/Brands/Services/Fred.php")).unwrap();
    let text = concat!("<?php\n", "namespace \n",);

    let items = complete_at(&backend, &uri, text, 1, 10).await;

    let inferred = items
        .iter()
        .find(|i| i.label == "Luxplus\\Core\\Brands\\Services")
        .expect("Should suggest Luxplus\\Core\\Brands\\Services from file path");

    assert_eq!(inferred.detail.as_deref(), Some("(from file path)"),);

    assert_eq!(inferred.preselect, Some(true),);

    // Should NOT infer Luxplus\Core\Database even though that prefix
    // exists — the file is not under src/database/.
    let db_inferred = items.iter().find(|i| {
        i.label == "Luxplus\\Core\\Database\\Brands\\Services"
            && i.detail.as_deref() == Some("(from file path)")
    });
    assert!(
        db_inferred.is_none(),
        "Should not infer namespace from non-matching PSR-4 base path"
    );
}

/// When the file is at the root of a PSR-4 source directory (e.g.
/// `src/Kernel.php`), the inferred namespace should be just the prefix.
#[tokio::test]
async fn test_namespace_inferred_at_source_root() {
    let (backend, dir) = create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": {
                    "App\\": "src/"
                }
            }
        }"#,
        &[],
    );

    std::fs::create_dir_all(dir.path().join("src")).unwrap();

    let uri = Url::from_file_path(dir.path().join("src/Kernel.php")).unwrap();
    let text = concat!("<?php\n", "namespace \n",);

    let items = complete_at(&backend, &uri, text, 1, 10).await;

    let inferred = items
        .iter()
        .find(|i| i.label == "App" && i.detail.as_deref() == Some("(from file path)"))
        .expect("Should suggest App inferred from src/Kernel.php");

    assert_eq!(inferred.preselect, Some(true));
}

/// When the file is not under any PSR-4 source directory, no inferred
/// namespace should appear (no items with the "(from file path)" detail).
#[tokio::test]
async fn test_namespace_no_inference_outside_psr4() {
    let (backend, dir) = create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": {
                    "App\\": "src/"
                }
            }
        }"#,
        &[],
    );

    std::fs::create_dir_all(dir.path().join("config")).unwrap();

    let uri = Url::from_file_path(dir.path().join("config/app.php")).unwrap();
    let text = concat!("<?php\n", "namespace \n",);

    let items = complete_at(&backend, &uri, text, 1, 10).await;

    let inferred: Vec<&CompletionItem> = items
        .iter()
        .filter(|i| i.detail.as_deref() == Some("(from file path)"))
        .collect();

    assert!(
        inferred.is_empty(),
        "Should not have any inferred namespaces for files outside PSR-4 dirs, got: {:?}",
        inferred.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
}

/// Inferred namespace should still work when the user has started typing
/// a partial namespace prefix that matches the inferred one.
#[tokio::test]
async fn test_namespace_inferred_with_partial_prefix() {
    let (backend, dir) = create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": {
                    "App\\": "src/"
                }
            }
        }"#,
        &[],
    );

    std::fs::create_dir_all(dir.path().join("src/Models")).unwrap();

    let uri = Url::from_file_path(dir.path().join("src/Models/User.php")).unwrap();
    let text = concat!("<?php\n", "namespace App\n",);

    // Cursor at end of "App" (col 13).
    let items = complete_at(&backend, &uri, text, 1, 13).await;

    let inferred = items
        .iter()
        .find(|i| i.label == "App\\Models" && i.detail.as_deref() == Some("(from file path)"));

    assert!(
        inferred.is_some(),
        "Typing partial 'App' should still show inferred App\\Models, got: {:?}",
        items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
}

/// Non-inferred namespace items should NOT be preselected and should not
/// have the "(from file path)" detail.
#[tokio::test]
async fn test_namespace_non_inferred_items_not_preselected() {
    let (backend, dir) = create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": {
                    "App\\": "src/",
                    "Tests\\": "tests/"
                }
            }
        }"#,
        &[],
    );

    std::fs::create_dir_all(dir.path().join("src/Models")).unwrap();

    let uri = Url::from_file_path(dir.path().join("src/Models/User.php")).unwrap();
    let text = concat!("<?php\n", "namespace \n",);

    let items = complete_at(&backend, &uri, text, 1, 10).await;

    // "Tests" should appear but not be preselected or marked as inferred.
    let tests_item = items
        .iter()
        .find(|i| i.label == "Tests")
        .expect("Should have Tests in namespace completions");

    assert_ne!(
        tests_item.preselect,
        Some(true),
        "Tests namespace should NOT be preselected when file is under src/"
    );
    assert_ne!(
        tests_item.detail.as_deref(),
        Some("(from file path)"),
        "Tests namespace should NOT have '(from file path)' detail"
    );
}
