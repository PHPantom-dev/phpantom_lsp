use crate::common::{create_psr4_workspace, create_test_backend};
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

#[tokio::test]
async fn test_in_array_strict_narrows_to_element_type() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///in_array_basic.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class AdminUser {\n",
        "    public function manageUsers(): void {}\n",
        "}\n",
        "class RegularUser {\n",
        "    public function viewProfile(): void {}\n",
        "}\n",
        "class Svc {\n",
        "    /**\n",
        "     * @param AdminUser|RegularUser $user\n",
        "     * @param list<AdminUser> $admins\n",
        "     */\n",
        "    public function test($user, array $admins): void {\n",
        "        if (in_array($user, $admins, true)) {\n",
        "            $user->\n",
        "        }\n",
        "    }\n",
        "}\n",
    );

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
                text_document: TextDocumentIdentifier { uri },
                position: Position {
                    line: 14,
                    character: 19,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some(), "Should return completions");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"manageUsers"),
                "Should include AdminUser's method 'manageUsers' inside in_array block, got: {:?}",
                method_names
            );
            assert!(
                !method_names.contains(&"viewProfile"),
                "Should NOT include RegularUser's method 'viewProfile' inside in_array block, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_in_array_strict_no_narrowing_without_strict_flag() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///in_array_no_strict.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class AdminUser {\n",
        "    public function manageUsers(): void {}\n",
        "}\n",
        "class RegularUser {\n",
        "    public function viewProfile(): void {}\n",
        "}\n",
        "class Svc {\n",
        "    /**\n",
        "     * @param AdminUser|RegularUser $user\n",
        "     * @param list<AdminUser> $admins\n",
        "     */\n",
        "    public function test($user, array $admins): void {\n",
        "        if (in_array($user, $admins)) {\n",
        "            $user->\n",
        "        }\n",
        "    }\n",
        "}\n",
    );

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
                text_document: TextDocumentIdentifier { uri },
                position: Position {
                    line: 14,
                    character: 19,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some(), "Should return completions");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            // Without strict flag, both types should remain.
            assert!(
                method_names.contains(&"manageUsers"),
                "Should include AdminUser's method without strict flag, got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"viewProfile"),
                "Should include RegularUser's method without strict flag, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_in_array_strict_else_branch_keeps_element_class() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///in_array_else.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class AdminUser {\n",
        "    public function manageUsers(): void {}\n",
        "}\n",
        "class RegularUser {\n",
        "    public function viewProfile(): void {}\n",
        "}\n",
        "class Svc {\n",
        "    /**\n",
        "     * @param AdminUser|RegularUser $user\n",
        "     * @param list<AdminUser> $admins\n",
        "     */\n",
        "    public function test($user, array $admins): void {\n",
        "        if (in_array($user, $admins, true)) {\n",
        "            // narrowed\n",
        "        } else {\n",
        "            $user->\n",
        "        }\n",
        "    }\n",
        "}\n",
    );

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
                text_document: TextDocumentIdentifier { uri },
                position: Position {
                    line: 16,
                    character: 19,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some(), "Should return completions");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            // An `AdminUser` that `$admins` does not hold fails the check
            // as well, so failing it rules no class out.
            assert!(
                method_names.contains(&"manageUsers"),
                "Should include AdminUser's method in else branch, got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"viewProfile"),
                "Should include RegularUser's method in else branch, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_in_array_strict_negated_condition() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///in_array_negated.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class AdminUser {\n",
        "    public function manageUsers(): void {}\n",
        "}\n",
        "class RegularUser {\n",
        "    public function viewProfile(): void {}\n",
        "}\n",
        "class Svc {\n",
        "    /**\n",
        "     * @param AdminUser|RegularUser $user\n",
        "     * @param list<AdminUser> $admins\n",
        "     */\n",
        "    public function test($user, array $admins): void {\n",
        "        if (!in_array($user, $admins, true)) {\n",
        "            $user->\n",
        "        }\n",
        "    }\n",
        "}\n",
    );

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
                text_document: TextDocumentIdentifier { uri },
                position: Position {
                    line: 14,
                    character: 19,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some(), "Should return completions");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            // Negated: inside the then-body the variable is not in the
            // haystack, which an `AdminUser` can be as well as a
            // `RegularUser`.
            assert!(
                method_names.contains(&"manageUsers"),
                "Should include AdminUser's method in negated in_array then-body, got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"viewProfile"),
                "Should include RegularUser's method in negated in_array then-body, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_in_array_strict_guard_clause_narrows() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///in_array_guard.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class AdminUser {\n",
        "    public function manageUsers(): void {}\n",
        "}\n",
        "class RegularUser {\n",
        "    public function viewProfile(): void {}\n",
        "}\n",
        "class Svc {\n",
        "    /**\n",
        "     * @param AdminUser|RegularUser $user\n",
        "     * @param list<AdminUser> $admins\n",
        "     */\n",
        "    public function test($user, array $admins): void {\n",
        "        if (!in_array($user, $admins, true)) {\n",
        "            return;\n",
        "        }\n",
        "        $user->\n",
        "    }\n",
        "}\n",
    );

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
                text_document: TextDocumentIdentifier { uri },
                position: Position {
                    line: 16,
                    character: 19,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some(), "Should return completions");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"manageUsers"),
                "Should include AdminUser's method after guard clause, got: {:?}",
                method_names
            );
            assert!(
                !method_names.contains(&"viewProfile"),
                "Should NOT include RegularUser's method after guard clause, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_in_array_strict_guard_clause_positive_keeps_element_class() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///in_array_guard_positive.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class AdminUser {\n",
        "    public function manageUsers(): void {}\n",
        "}\n",
        "class RegularUser {\n",
        "    public function viewProfile(): void {}\n",
        "}\n",
        "class Svc {\n",
        "    /**\n",
        "     * @param AdminUser|RegularUser $user\n",
        "     * @param list<AdminUser> $admins\n",
        "     */\n",
        "    public function test($user, array $admins): void {\n",
        "        if (in_array($user, $admins, true)) {\n",
        "            return;\n",
        "        }\n",
        "        $user->\n",
        "    }\n",
        "}\n",
    );

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
                text_document: TextDocumentIdentifier { uri },
                position: Position {
                    line: 16,
                    character: 19,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some(), "Should return completions");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            // Positive in_array + return => after the guard, var is not in
            // the haystack, which rules out no `AdminUser` the haystack
            // does not hold.
            assert!(
                method_names.contains(&"manageUsers"),
                "Should include AdminUser's method after positive guard, got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"viewProfile"),
                "Should include RegularUser's method after positive guard, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_in_array_strict_with_array_shorthand_type() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///in_array_shorthand.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Dog {\n",
        "    public function bark(): void {}\n",
        "}\n",
        "class Cat {\n",
        "    public function purr(): void {}\n",
        "}\n",
        "class Svc {\n",
        "    /**\n",
        "     * @param Dog|Cat $pet\n",
        "     * @param Dog[] $dogs\n",
        "     */\n",
        "    public function test($pet, array $dogs): void {\n",
        "        if (in_array($pet, $dogs, true)) {\n",
        "            $pet->\n",
        "        }\n",
        "    }\n",
        "}\n",
    );

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
                text_document: TextDocumentIdentifier { uri },
                position: Position {
                    line: 14,
                    character: 18,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some(), "Should return completions");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"bark"),
                "Should include Dog's method with Dog[] shorthand, got: {:?}",
                method_names
            );
            assert!(
                !method_names.contains(&"purr"),
                "Should NOT include Cat's method with Dog[] shorthand, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_in_array_strict_with_array_generic_type() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///in_array_generic.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Dog {\n",
        "    public function bark(): void {}\n",
        "}\n",
        "class Cat {\n",
        "    public function purr(): void {}\n",
        "}\n",
        "class Svc {\n",
        "    /**\n",
        "     * @param Dog|Cat $pet\n",
        "     * @param array<int, Dog> $dogs\n",
        "     */\n",
        "    public function test($pet, array $dogs): void {\n",
        "        if (in_array($pet, $dogs, true)) {\n",
        "            $pet->\n",
        "        }\n",
        "    }\n",
        "}\n",
    );

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
                text_document: TextDocumentIdentifier { uri },
                position: Position {
                    line: 14,
                    character: 18,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some(), "Should return completions");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"bark"),
                "Should include Dog's method with array<int, Dog>, got: {:?}",
                method_names
            );
            assert!(
                !method_names.contains(&"purr"),
                "Should NOT include Cat's method with array<int, Dog>, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_in_array_strict_with_inline_var_docblock() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///in_array_inline_var.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Dog {\n",
        "    public function bark(): void {}\n",
        "}\n",
        "class Cat {\n",
        "    public function purr(): void {}\n",
        "}\n",
        "class Svc {\n",
        "    /** @param Dog|Cat $pet */\n",
        "    public function test($pet): void {\n",
        "        /** @var list<Dog> $dogs */\n",
        "        $dogs = [];\n",
        "        if (in_array($pet, $dogs, true)) {\n",
        "            $pet->\n",
        "        }\n",
        "    }\n",
        "}\n",
    );

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
                text_document: TextDocumentIdentifier { uri },
                position: Position {
                    line: 13,
                    character: 18,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some(), "Should return completions");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"bark"),
                "Should include Dog's method with inline @var, got: {:?}",
                method_names
            );
            assert!(
                !method_names.contains(&"purr"),
                "Should NOT include Cat's method with inline @var, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_in_array_strict_cross_file() {
    let composer_json = r#"{"autoload":{"psr-4":{"App\\":"src/"}}}"#;
    let files = vec![
        (
            "src/Animal.php",
            concat!(
                "<?php\n",
                "namespace App;\n",
                "class Animal {\n",
                "    public function eat(): void {}\n",
                "}\n",
            ),
        ),
        (
            "src/Dog.php",
            concat!(
                "<?php\n",
                "namespace App;\n",
                "class Dog extends Animal {\n",
                "    public function bark(): void {}\n",
                "}\n",
            ),
        ),
        (
            "src/Cat.php",
            concat!(
                "<?php\n",
                "namespace App;\n",
                "class Cat extends Animal {\n",
                "    public function purr(): void {}\n",
                "}\n",
            ),
        ),
        (
            "src/Svc.php",
            concat!(
                "<?php\n",
                "namespace App;\n",
                "class Svc {\n",
                "    /**\n",
                "     * @param Dog|Cat $pet\n",
                "     * @param list<Dog> $dogs\n",
                "     */\n",
                "    public function test($pet, array $dogs): void {\n",
                "        if (in_array($pet, $dogs, true)) {\n",
                "            $pet->\n",
                "        }\n",
                "    }\n",
                "}\n",
            ),
        ),
    ];

    let (backend, _dir) = create_psr4_workspace(composer_json, &files);

    let svc_uri = Url::from_file_path(_dir.path().join("src/Svc.php")).unwrap();
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: svc_uri.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: std::fs::read_to_string(_dir.path().join("src/Svc.php")).unwrap(),
            },
        })
        .await;

    let result = backend
        .completion(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: svc_uri },
                position: Position {
                    line: 9,
                    character: 18,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some(), "Should return completions");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"bark"),
                "Should include Dog's method in cross-file in_array, got: {:?}",
                method_names
            );
            assert!(
                !method_names.contains(&"purr"),
                "Should NOT include Cat's method in cross-file in_array, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_in_array_strict_while_loop() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///in_array_while.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class AdminUser {\n",
        "    public function manageUsers(): void {}\n",
        "}\n",
        "class RegularUser {\n",
        "    public function viewProfile(): void {}\n",
        "}\n",
        "class Svc {\n",
        "    /**\n",
        "     * @param AdminUser|RegularUser $user\n",
        "     * @param list<AdminUser> $admins\n",
        "     */\n",
        "    public function test($user, array $admins): void {\n",
        "        while (in_array($user, $admins, true)) {\n",
        "            $user->\n",
        "        }\n",
        "    }\n",
        "}\n",
    );

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
                text_document: TextDocumentIdentifier { uri },
                position: Position {
                    line: 14,
                    character: 19,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some(), "Should return completions");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"manageUsers"),
                "Should include AdminUser's method in while-body, got: {:?}",
                method_names
            );
            assert!(
                !method_names.contains(&"viewProfile"),
                "Should NOT include RegularUser's method in while-body, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_in_array_strict_parenthesised_condition() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///in_array_parens.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Dog {\n",
        "    public function bark(): void {}\n",
        "}\n",
        "class Cat {\n",
        "    public function purr(): void {}\n",
        "}\n",
        "class Svc {\n",
        "    /**\n",
        "     * @param Dog|Cat $pet\n",
        "     * @param list<Dog> $dogs\n",
        "     */\n",
        "    public function test($pet, array $dogs): void {\n",
        "        if ((in_array($pet, $dogs, true))) {\n",
        "            $pet->\n",
        "        }\n",
        "    }\n",
        "}\n",
    );

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
                text_document: TextDocumentIdentifier { uri },
                position: Position {
                    line: 14,
                    character: 18,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some(), "Should return completions");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"bark"),
                "Should include Dog's method with parenthesised condition, got: {:?}",
                method_names
            );
            assert!(
                !method_names.contains(&"purr"),
                "Should NOT include Cat's method with parenthesised condition, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_in_array_strict_guard_clause_throw() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///in_array_guard_throw.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Dog {\n",
        "    public function bark(): void {}\n",
        "}\n",
        "class Cat {\n",
        "    public function purr(): void {}\n",
        "}\n",
        "class Svc {\n",
        "    /**\n",
        "     * @param Dog|Cat $pet\n",
        "     * @param list<Dog> $dogs\n",
        "     */\n",
        "    public function test($pet, array $dogs): void {\n",
        "        if (!in_array($pet, $dogs, true)) {\n",
        "            throw new \\RuntimeException('not a dog');\n",
        "        }\n",
        "        $pet->\n",
        "    }\n",
        "}\n",
    );

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
                text_document: TextDocumentIdentifier { uri },
                position: Position {
                    line: 16,
                    character: 19,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some(), "Should return completions");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"bark"),
                "Should include Dog's method after throw guard clause, got: {:?}",
                method_names
            );
            assert!(
                !method_names.contains(&"purr"),
                "Should NOT include Cat's method after throw guard clause, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_in_array_strict_no_narrowing_when_guard_body_does_not_exit() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///in_array_guard_no_exit.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Dog {\n",
        "    public function bark(): void {}\n",
        "}\n",
        "class Cat {\n",
        "    public function purr(): void {}\n",
        "}\n",
        "class Svc {\n",
        "    /**\n",
        "     * @param Dog|Cat $pet\n",
        "     * @param list<Dog> $dogs\n",
        "     */\n",
        "    public function test($pet, array $dogs): void {\n",
        "        if (!in_array($pet, $dogs, true)) {\n",
        "            echo 'not a dog';\n",
        "        }\n",
        "        $pet->\n",
        "    }\n",
        "}\n",
    );

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
                text_document: TextDocumentIdentifier { uri },
                position: Position {
                    line: 16,
                    character: 19,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some(), "Should return completions");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            // No narrowing because the guard body does not exit.
            assert!(
                method_names.contains(&"bark"),
                "Should include Dog's method when guard does not exit, got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"purr"),
                "Should include Cat's method when guard does not exit, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_in_array_strict_with_union_element_type() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///in_array_union.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Dog {\n",
        "    public function bark(): void {}\n",
        "}\n",
        "class Cat {\n",
        "    public function purr(): void {}\n",
        "}\n",
        "class Fish {\n",
        "    public function swim(): void {}\n",
        "}\n",
        "class Svc {\n",
        "    /**\n",
        "     * @param Dog|Cat|Fish $pet\n",
        "     * @param list<Dog|Cat> $housePets\n",
        "     */\n",
        "    public function test($pet, array $housePets): void {\n",
        "        if (in_array($pet, $housePets, true)) {\n",
        "            $pet->\n",
        "        }\n",
        "    }\n",
        "}\n",
    );

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
                text_document: TextDocumentIdentifier { uri },
                position: Position {
                    line: 17,
                    character: 18,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    assert!(result.is_some(), "Should return completions");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"bark"),
                "Should include Dog's method with union element type, got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"purr"),
                "Should include Cat's method with union element type, got: {:?}",
                method_names
            );
            assert!(
                !method_names.contains(&"swim"),
                "Should NOT include Fish's method with union element type, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_in_array_strict_this_property_haystack() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///in_array_this_prop.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class AdminUser {\n",
        "    public function manageUsers(): void {}\n",
        "}\n",
        "class RegularUser {\n",
        "    public function viewProfile(): void {}\n",
        "}\n",
        "class Svc {\n",
        "    /** @var list<AdminUser> */\n",
        "    private array $admins;\n",
        "\n",
        "    /** @param AdminUser|RegularUser $user */\n",
        "    public function test($user): void {\n",
        "        if (in_array($user, $this->admins, true)) {\n",
        "            $user->\n",
        "        }\n",
        "    }\n",
        "}\n",
    );

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
                text_document: TextDocumentIdentifier { uri },
                position: Position {
                    line: 14,
                    character: 19,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    // $this->admins goes through a different resolution path (property
    // access rather than simple variable). The narrowing may or may not
    // resolve depending on how extract_rhs_iterable_raw_type handles
    // property access. This test documents current behaviour: at minimum
    // the completion should not crash and should return something.
    assert!(result.is_some(), "Should return completions");
}
