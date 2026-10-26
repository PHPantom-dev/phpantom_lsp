use crate::common::{completion_response, create_test_backend, open_php};
use tower_lsp_server::LanguageServer;
use tower_lsp_server::gen_lsp_types::*;

#[tokio::test]
async fn test_initialize_server_info() {
    let backend = create_test_backend();
    let params = InitializeParams::default();
    let result = backend.initialize(params).await.unwrap();

    let server_info = result.server_info.expect("server_info should be present");
    assert_eq!(server_info.name, "PHPantom");
    assert_eq!(
        server_info.version,
        Some(env!("PHPANTOM_GIT_VERSION").to_string())
    );
}

#[tokio::test]
async fn test_initialize_capabilities() {
    let backend = create_test_backend();
    let params = InitializeParams::default();
    let result = backend.initialize(params).await.unwrap();

    let caps = result.capabilities;
    assert!(
        caps.completion_provider.is_some(),
        "Completion provider should be enabled"
    );
}

#[tokio::test]
async fn test_initialize_advertises_type_hierarchy_statically() {
    let backend = create_test_backend();
    let result = backend
        .initialize(InitializeParams::default())
        .await
        .unwrap();

    assert_eq!(
        result.capabilities.type_hierarchy_provider,
        Some(TypeHierarchyProvider::Bool(true)),
        "a client without dynamic registration needs the static capability"
    );
}

#[tokio::test]
async fn test_initialize_leaves_type_hierarchy_to_dynamic_registration() {
    let backend = create_test_backend();
    let params = InitializeParams {
        capabilities: ClientCapabilities {
            text_document: Some(TextDocumentClientCapabilities {
                type_hierarchy: Some(TypeHierarchyClientCapabilities {
                    dynamic_registration: Some(true),
                }),
                ..TextDocumentClientCapabilities::default()
            }),
            ..ClientCapabilities::default()
        },
        ..InitializeParams::default()
    };
    let result = backend.initialize(params).await.unwrap();

    assert_eq!(
        result.capabilities.type_hierarchy_provider, None,
        "a client that registers type hierarchy dynamically must not also get it statically"
    );
}

#[tokio::test]
async fn test_did_open_stores_file() {
    let backend = create_test_backend();

    let uri = Uri::parse("file:///test.php").unwrap();
    let text = "<?php\nclass Stored { function m() {} }\n".to_string();

    open_php(&backend, &uri, &text).await;

    // Verify the file was stored by checking the AST map has an entry
    let classes = backend.get_classes_for_uri(uri.as_ref());
    assert!(
        classes.is_some(),
        "AST map should have an entry after did_open"
    );
    assert_eq!(classes.unwrap().len(), 1);
}

#[tokio::test]
async fn test_completion_returns_none_when_nothing_matches() {
    let backend = create_test_backend();

    let uri = Uri::parse("file:///test.php").unwrap();
    let text = "<?php\n$x = 1;\n".to_string();

    open_php(&backend, &uri, &text).await;

    let result = completion_response(&backend, &uri, 1, 0, None).await;
    assert!(
        result.is_none(),
        "Completion should return None when nothing matches"
    );
}

#[tokio::test]
async fn test_shutdown() {
    let backend = create_test_backend();
    let result = backend.shutdown().await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_did_change_updates_content() {
    let backend = create_test_backend();

    let uri = Uri::parse("file:///test.php").unwrap();
    let initial_text = "<?php\nclass A { function first() {} }\n".to_string();

    open_php(&backend, &uri, &initial_text).await;

    let classes = backend.get_classes_for_uri(uri.as_ref()).unwrap();
    assert_eq!(classes[0].methods.len(), 1);

    // Change the content to add a second method
    let change_params = DidChangeTextDocumentParams {
        text_document: VersionedTextDocumentIdentifier {
            text_document_identifier: TextDocumentIdentifier { uri: uri.clone() },
            version: 2,
        },
        content_changes: vec![
            TextDocumentContentChangeEvent::TextDocumentContentChangeWholeDocument(
                TextDocumentContentChangeWholeDocument {
                    text: "<?php\nclass A { function first() {} function second() {} }\n"
                        .to_string(),
                },
            ),
        ],
    };
    backend.did_change(change_params).await;

    // Verify content was updated by checking the re-parsed AST
    let classes = backend.get_classes_for_uri(uri.as_ref()).unwrap();
    assert_eq!(
        classes[0].methods.len(),
        2,
        "After change, class should have 2 methods"
    );
}

#[tokio::test]
async fn test_did_close_removes_file() {
    let backend = create_test_backend();

    let uri = Uri::parse("file:///test.php").unwrap();
    let text = "<?php\nclass Z { function z() {} }\n".to_string();

    open_php(&backend, &uri, &text).await;

    assert!(backend.get_classes_for_uri(uri.as_ref()).is_some());

    // Close the file
    let close_params = DidCloseTextDocumentParams {
        text_document: TextDocumentIdentifier { uri: uri.clone() },
    };
    backend.did_close(close_params).await;

    // ast_map is dropped on close to free memory, but class_index
    // is kept so GTD can still locate the file.
    assert!(
        backend.get_classes_for_uri(uri.as_ref()).is_none(),
        "After close, AST map should be cleared"
    );
    assert!(
        backend.fqn_uri_index().read().contains_key("Z"),
        "class_index should retain entries after close"
    );
}

#[tokio::test]
async fn test_did_open_populates_ast_map() {
    let backend = create_test_backend();

    let uri = Uri::parse("file:///user.php").unwrap();
    let text =
        "<?php\nclass User {\n    function login() {}\n    function logout() {}\n}\n".to_string();

    open_php(&backend, &uri, &text).await;

    let classes = backend
        .get_classes_for_uri(uri.as_ref())
        .expect("ast_map should have entry for URI");
    assert_eq!(classes.len(), 1);
    assert_eq!(classes[0].name, "User");
    assert_eq!(classes[0].methods.len(), 2);

    let method_names: Vec<&str> = classes[0].methods.iter().map(|m| m.name.as_str()).collect();
    assert!(method_names.contains(&"login"));
    assert!(method_names.contains(&"logout"));
}

#[tokio::test]
async fn test_did_change_reparses_ast() {
    let backend = create_test_backend();

    let uri = Uri::parse("file:///changing.php").unwrap();

    // Open with initial content: one class with one method
    open_php(
        &backend,
        &uri,
        "<?php\nclass A {\n    function first() {}\n}\n",
    )
    .await;

    let classes = backend.get_classes_for_uri(uri.as_ref()).unwrap();
    assert_eq!(classes[0].methods.len(), 1);
    assert_eq!(classes[0].methods[0].name, "first");

    // Change the file: add a second method
    let change_params = DidChangeTextDocumentParams {
        text_document: VersionedTextDocumentIdentifier {
            text_document_identifier: TextDocumentIdentifier { uri: uri.clone() },
            version: 2,
        },
        content_changes: vec![
            TextDocumentContentChangeEvent::TextDocumentContentChangeWholeDocument(
                TextDocumentContentChangeWholeDocument {
                    text:
                        "<?php\nclass A {\n    function first() {}\n    function second() {}\n}\n"
                            .to_string(),
                },
            ),
        ],
    };
    backend.did_change(change_params).await;

    // Verify the AST was re-parsed
    let classes = backend.get_classes_for_uri(uri.as_ref()).unwrap();
    assert_eq!(classes[0].methods.len(), 2);
    let method_names: Vec<&str> = classes[0].methods.iter().map(|m| m.name.as_str()).collect();
    assert!(method_names.contains(&"first"));
    assert!(method_names.contains(&"second"));

    // Verify the AST was re-parsed and has both methods
    let classes = backend.get_classes_for_uri(uri.as_ref()).unwrap();
    assert_eq!(classes[0].methods.len(), 2);
    let method_names: Vec<&str> = classes[0].methods.iter().map(|m| m.name.as_str()).collect();
    assert!(method_names.contains(&"first"));
    assert!(method_names.contains(&"second"));
}

#[tokio::test]
async fn test_did_close_cleans_up_ast_map() {
    let backend = create_test_backend();

    let uri = Uri::parse("file:///cleanup.php").unwrap();
    let text = "<?php\nclass X {\n    function y() {}\n}\n".to_string();

    // Open
    open_php(&backend, &uri, &text).await;

    // Verify ast_map is populated
    assert!(backend.get_classes_for_uri(uri.as_ref()).is_some());

    // Close
    let close_params = DidCloseTextDocumentParams {
        text_document: TextDocumentIdentifier { uri: uri.clone() },
    };
    backend.did_close(close_params).await;

    // Verify ast_map entry is dropped on close to free memory
    assert!(
        backend.get_classes_for_uri(uri.as_ref()).is_none(),
        "ast_map should be cleaned up after did_close"
    );
}

#[tokio::test]
async fn test_did_change_incremental_sync() {
    let backend = create_test_backend();

    let uri = Uri::parse("file:///incremental.php").unwrap();
    //                     0123456789...
    let initial = "<?php\nclass A {\n    function first() {}\n}\n".to_string();

    open_php(&backend, &uri, &initial).await;

    let classes = backend.get_classes_for_uri(uri.as_ref()).unwrap();
    assert_eq!(classes[0].methods.len(), 1);

    // Send an incremental change: insert a second method before the closing brace.
    // The closing "}\n" is at line 3, col 0. Insert before it.
    let change_params = DidChangeTextDocumentParams {
        text_document: VersionedTextDocumentIdentifier {
            text_document_identifier: TextDocumentIdentifier { uri: uri.clone() },
            version: 2,
        },
        content_changes: vec![
            TextDocumentContentChangeEvent::TextDocumentContentChangePartial(
                TextDocumentContentChangePartial {
                    range: Range {
                        start: Position {
                            line: 3,
                            character: 0,
                        },
                        end: Position {
                            line: 3,
                            character: 0,
                        },
                    },
                    text: "    function second() {}\n".to_string(),
                    ..Default::default()
                },
            ),
        ],
    };
    backend.did_change(change_params).await;

    let classes = backend.get_classes_for_uri(uri.as_ref()).unwrap();
    assert_eq!(classes[0].methods.len(), 2);
    let names: Vec<&str> = classes[0].methods.iter().map(|m| m.name.as_str()).collect();
    assert!(names.contains(&"first"));
    assert!(names.contains(&"second"));
}

#[tokio::test]
async fn test_did_change_incremental_multiple_edits() {
    let backend = create_test_backend();

    let uri = Uri::parse("file:///multi_edit.php").unwrap();
    let initial = "<?php\nclass B {\n    function alpha() {}\n}\n".to_string();

    open_php(&backend, &uri, &initial).await;

    // Two incremental edits in one event: rename "alpha" to "beta" and add a method.
    // "alpha" starts at line 2, char 13, ends at char 18.
    // After that edit the file is: "<?php\nclass B {\n    function beta() {}\n}\n"
    // Then insert before closing brace (now line 3, char 0).
    let change_params = DidChangeTextDocumentParams {
        text_document: VersionedTextDocumentIdentifier {
            text_document_identifier: TextDocumentIdentifier { uri: uri.clone() },
            version: 2,
        },
        content_changes: vec![
            TextDocumentContentChangeEvent::TextDocumentContentChangePartial(
                TextDocumentContentChangePartial {
                    range: Range {
                        start: Position {
                            line: 2,
                            character: 13,
                        },
                        end: Position {
                            line: 2,
                            character: 18,
                        },
                    },
                    text: "beta".to_string(),
                    ..Default::default()
                },
            ),
            TextDocumentContentChangeEvent::TextDocumentContentChangePartial(
                TextDocumentContentChangePartial {
                    range: Range {
                        start: Position {
                            line: 3,
                            character: 0,
                        },
                        end: Position {
                            line: 3,
                            character: 0,
                        },
                    },
                    text: "    function gamma() {}\n".to_string(),
                    ..Default::default()
                },
            ),
        ],
    };
    backend.did_change(change_params).await;

    let classes = backend.get_classes_for_uri(uri.as_ref()).unwrap();
    assert_eq!(classes[0].methods.len(), 2);
    let names: Vec<&str> = classes[0].methods.iter().map(|m| m.name.as_str()).collect();
    assert!(names.contains(&"beta"));
    assert!(names.contains(&"gamma"));
    assert!(!names.contains(&"alpha"));
}
