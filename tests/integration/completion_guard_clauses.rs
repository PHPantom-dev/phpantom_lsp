use crate::common::{completion_response, create_psr4_workspace, create_test_backend, open_php};
use tower_lsp::lsp_types::*;

#[tokio::test]
async fn test_guard_clause_negated_instanceof_return_narrows() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///guard_neg_instanceof.php").unwrap();
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
        "        if (!$pet instanceof Dog) {\n",
        "            return;\n",
        "        }\n",
        "        $pet->\n",
        "    }\n",
        "}\n",
    );

    open_php(&backend, &uri, text).await;

    let result = completion_response(&backend, &uri, 13, 14, None).await;

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
                "Should include Dog's method 'bark' after guard clause, got: {:?}",
                method_names
            );
            assert!(
                !method_names.contains(&"purr"),
                "Should NOT include Cat's method 'purr' after guard clause, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_guard_clause_positive_instanceof_return_excludes() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///guard_pos_instanceof.php").unwrap();
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
        "        if ($pet instanceof Dog) {\n",
        "            return;\n",
        "        }\n",
        "        $pet->\n",
        "    }\n",
        "}\n",
    );

    open_php(&backend, &uri, text).await;

    let result = completion_response(&backend, &uri, 13, 14, None).await;

    assert!(result.is_some(), "Should return completions");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"purr"),
                "Should include Cat's method 'purr' after guard excludes Dog, got: {:?}",
                method_names
            );
            assert!(
                !method_names.contains(&"bark"),
                "Should NOT include Dog's method 'bark' after guard excludes Dog, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_guard_clause_throw_narrows() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///guard_throw.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class User {\n",
        "    public function getEmail(): string { return ''; }\n",
        "}\n",
        "class Admin {\n",
        "    public function getRole(): string { return ''; }\n",
        "}\n",
        "class Svc {\n",
        "    /** @param User|Admin $u */\n",
        "    public function test($u): void {\n",
        "        if ($u instanceof Admin) {\n",
        "            throw new \\Exception('no admins');\n",
        "        }\n",
        "        $u->\n",
        "    }\n",
        "}\n",
    );

    open_php(&backend, &uri, text).await;

    let result = completion_response(&backend, &uri, 13, 12, None).await;

    assert!(result.is_some(), "Should return completions");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"getEmail"),
                "Should include User's 'getEmail' after throw guard, got: {:?}",
                method_names
            );
            assert!(
                !method_names.contains(&"getRole"),
                "Should NOT include Admin's 'getRole' after throw guard, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_guard_clause_multiple_sequential_guards() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///guard_multiple.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Alpha {\n",
        "    public function alphaMethod(): void {}\n",
        "}\n",
        "class Beta {\n",
        "    public function betaMethod(): void {}\n",
        "}\n",
        "class Gamma {\n",
        "    public function gammaMethod(): void {}\n",
        "}\n",
        "class Svc {\n",
        "    /** @param Alpha|Beta|Gamma $obj */\n",
        "    public function test($obj): void {\n",
        "        if ($obj instanceof Alpha) {\n",
        "            return;\n",
        "        }\n",
        "        if ($obj instanceof Beta) {\n",
        "            return;\n",
        "        }\n",
        "        $obj->\n",
        "    }\n",
        "}\n",
    );

    open_php(&backend, &uri, text).await;

    let result = completion_response(&backend, &uri, 19, 14, None).await;

    assert!(result.is_some(), "Should return completions");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"gammaMethod"),
                "Should include Gamma's method after two guard clauses, got: {:?}",
                method_names
            );
            assert!(
                !method_names.contains(&"alphaMethod"),
                "Should NOT include Alpha's method after guard, got: {:?}",
                method_names
            );
            assert!(
                !method_names.contains(&"betaMethod"),
                "Should NOT include Beta's method after guard, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_guard_clause_no_narrowing_when_body_does_not_exit() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///guard_no_exit.php").unwrap();
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
        "        if (!$pet instanceof Dog) {\n",
        "            echo 'not a dog';\n",
        "        }\n",
        "        $pet->\n",
        "    }\n",
        "}\n",
    );

    open_php(&backend, &uri, text).await;

    let result = completion_response(&backend, &uri, 13, 14, None).await;

    assert!(result.is_some(), "Should return completions");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            // Body doesn't exit, so no guard clause narrowing applies
            assert!(
                method_names.contains(&"bark"),
                "Should include Dog's 'bark' — body doesn't exit, got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"purr"),
                "Should include Cat's 'purr' — body doesn't exit, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_guard_clause_no_narrowing_when_else_exists() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///guard_with_else.php").unwrap();
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
        "        if (!$pet instanceof Dog) {\n",
        "            return;\n",
        "        } else {\n",
        "            echo 'is dog';\n",
        "        }\n",
        "        $pet->\n",
        "    }\n",
        "}\n",
    );

    open_php(&backend, &uri, text).await;

    let result = completion_response(&backend, &uri, 15, 14, None).await;

    assert!(result.is_some(), "Should return completions");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            // The forward walker correctly resolves to only Dog here: the cursor
            // is after the if/else, and the guard clause (`!$pet instanceof Dog`
            // → return) eliminates Cat, leaving only Dog visible.
            assert!(
                method_names.contains(&"bark"),
                "Should include Dog's 'bark' — forward walker narrows to Dog, got: {:?}",
                method_names
            );
            assert!(
                !method_names.contains(&"purr"),
                "Should NOT include Cat's 'purr' — forward walker excludes Cat after guard clause, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_guard_clause_top_level_function() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///guard_top_level.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class User {\n",
        "    public function getEmail(): string { return ''; }\n",
        "}\n",
        "class Guest {\n",
        "    public function getSession(): string { return ''; }\n",
        "}\n",
        "/** @param User|Guest $person */\n",
        "function process($person): void {\n",
        "    if (!$person instanceof User) {\n",
        "        return;\n",
        "    }\n",
        "    $person->\n",
        "}\n",
    );

    open_php(&backend, &uri, text).await;

    let result = completion_response(&backend, &uri, 12, 13, None).await;

    assert!(result.is_some(), "Should return completions");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"getEmail"),
                "Should include User's 'getEmail' after guard in top-level function, got: {:?}",
                method_names
            );
            assert!(
                !method_names.contains(&"getSession"),
                "Should NOT include Guest's 'getSession' after guard, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_guard_clause_cross_file() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{"autoload":{"psr-4":{"App\\":"src/"}}}"#,
        &[
            (
                "src/Animal.php",
                concat!(
                    "<?php\n",
                    "namespace App;\n",
                    "class Animal {\n",
                    "    public function breathe(): void {}\n",
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
        ],
    );

    let uri = Url::parse("file:///guard_cross_file.php").unwrap();
    let text = concat!(
        "<?php\n",
        "use App\\Dog;\n",
        "use App\\Cat;\n",
        "class Svc {\n",
        "    /** @param Dog|Cat $pet */\n",
        "    public function test($pet): void {\n",
        "        if (!$pet instanceof Dog) {\n",
        "            return;\n",
        "        }\n",
        "        $pet->\n",
        "    }\n",
        "}\n",
    );

    open_php(&backend, &uri, text).await;

    let result = completion_response(&backend, &uri, 9, 14, None).await;

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
                "Should include Dog's 'bark' after guard (cross-file), got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"breathe"),
                "Should include inherited 'breathe' after guard (cross-file), got: {:?}",
                method_names
            );
            assert!(
                !method_names.contains(&"purr"),
                "Should NOT include Cat's 'purr' after guard (cross-file), got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_guard_clause_parenthesised_condition() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///guard_parens.php").unwrap();
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
        "        if (!($pet instanceof Dog)) {\n",
        "            return;\n",
        "        }\n",
        "        $pet->\n",
        "    }\n",
        "}\n",
    );

    open_php(&backend, &uri, text).await;

    let result = completion_response(&backend, &uri, 13, 14, None).await;

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
                "Should include Dog's 'bark' after parenthesised guard, got: {:?}",
                method_names
            );
            assert!(
                !method_names.contains(&"purr"),
                "Should NOT include Cat's 'purr' after parenthesised guard, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_guard_clause_is_a_narrows() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///guard_is_a.php").unwrap();
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
        "        if (!is_a($pet, Dog::class)) {\n",
        "            return;\n",
        "        }\n",
        "        $pet->\n",
        "    }\n",
        "}\n",
    );

    open_php(&backend, &uri, text).await;

    let result = completion_response(&backend, &uri, 13, 14, None).await;

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
                "Should include Dog's 'bark' after is_a guard clause, got: {:?}",
                method_names
            );
            assert!(
                !method_names.contains(&"purr"),
                "Should NOT include Cat's 'purr' after is_a guard clause, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_guard_clause_no_narrowing_inside_if_body() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///guard_inside_body.php").unwrap();
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
        "        if (!$pet instanceof Dog) {\n",
        "            $pet->\n",
        "        }\n",
        "    }\n",
        "}\n",
    );

    open_php(&backend, &uri, text).await;

    let result = completion_response(&backend, &uri, 11, 18, None).await;

    assert!(result.is_some(), "Should return completions");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            // Inside the if body: $pet is NOT Dog, so should see Cat's methods
            assert!(
                method_names.contains(&"purr"),
                "Inside negated instanceof block should include Cat's 'purr', got: {:?}",
                method_names
            );
            assert!(
                !method_names.contains(&"bark"),
                "Inside negated instanceof block should NOT include Dog's 'bark', got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_guard_clause_mixed_type_parameter() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///guard_mixed.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class User {\n",
        "    public function getEmail(): string { return ''; }\n",
        "}\n",
        "class Svc {\n",
        "    public function process(mixed $value): void {\n",
        "        if (!$value instanceof User) {\n",
        "            return;\n",
        "        }\n",
        "        $value->\n",
        "    }\n",
        "}\n",
    );

    open_php(&backend, &uri, text).await;

    let result = completion_response(&backend, &uri, 9, 16, None).await;

    assert!(result.is_some(), "Should return completions");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            assert!(
                method_names.contains(&"getEmail"),
                "Should include User's 'getEmail' after guard on mixed param, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_guard_clause_with_block_body() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///guard_block.php").unwrap();
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
        "        if (!$pet instanceof Dog) {\n",
        "            $x = 1;\n",
        "            $y = 2;\n",
        "            return;\n",
        "        }\n",
        "        $pet->\n",
        "    }\n",
        "}\n",
    );

    open_php(&backend, &uri, text).await;

    let result = completion_response(&backend, &uri, 15, 14, None).await;

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
                "Should include Dog's 'bark' — block ends with return, got: {:?}",
                method_names
            );
            assert!(
                !method_names.contains(&"purr"),
                "Should NOT include Cat's 'purr' — block ends with return, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_guard_clause_no_narrowing_when_block_does_not_end_with_exit() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///guard_block_no_exit.php").unwrap();
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
        "        if (!$pet instanceof Dog) {\n",
        "            $x = 1;\n",
        "        }\n",
        "        $pet->\n",
        "    }\n",
        "}\n",
    );

    open_php(&backend, &uri, text).await;

    let result = completion_response(&backend, &uri, 13, 14, None).await;

    assert!(result.is_some(), "Should return completions");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap())
                .collect();

            // Block doesn't end with exit, so no narrowing
            assert!(
                method_names.contains(&"bark"),
                "Should include Dog's 'bark' — block doesn't exit, got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"purr"),
                "Should include Cat's 'purr' — block doesn't exit, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_guard_clause_single_statement_no_braces() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///guard_no_braces.php").unwrap();
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
        "        if (!$pet instanceof Dog) return;\n",
        "        $pet->\n",
        "    }\n",
        "}\n",
    );

    open_php(&backend, &uri, text).await;

    let result = completion_response(&backend, &uri, 11, 14, None).await;

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
                "Should include Dog's 'bark' after braceless guard, got: {:?}",
                method_names
            );
            assert!(
                !method_names.contains(&"purr"),
                "Should NOT include Cat's 'purr' after braceless guard, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_guard_clause_this_property() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///guard_this_prop.php").unwrap();
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
        "        if (!$pet instanceof Dog) {\n",
        "            throw new \\RuntimeException('Expected Dog');\n",
        "        }\n",
        "        $pet->\n",
        "    }\n",
        "}\n",
    );

    open_php(&backend, &uri, text).await;

    let result = completion_response(&backend, &uri, 13, 14, None).await;

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
                "Should include Dog's 'bark' after throw guard, got: {:?}",
                method_names
            );
            assert!(
                !method_names.contains(&"purr"),
                "Should NOT include Cat's 'purr' after throw guard, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_guard_clause_get_class_identity_check() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///guard_get_class.php").unwrap();
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
        "        if (get_class($pet) !== Dog::class) {\n",
        "            return;\n",
        "        }\n",
        "        $pet->\n",
        "    }\n",
        "}\n",
    );

    open_php(&backend, &uri, text).await;

    let result = completion_response(&backend, &uri, 13, 14, None).await;

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
                "Should include Dog's 'bark' after get_class guard, got: {:?}",
                method_names
            );
            assert!(
                !method_names.contains(&"purr"),
                "Should NOT include Cat's 'purr' after get_class guard, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── Null / falsy guard clause narrowing ────────────────────────────────────

/// `if (!$var) { continue; }` should narrow `$var` to non-null after the guard.
#[tokio::test]
async fn test_guard_clause_falsy_continue_narrows_null() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///guard_falsy_continue.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class OrderLine {\n",
        "    public int $actualAmount;\n",
        "}\n",
        "class Svc {\n",
        "    /** @param array<int, OrderLine|null> $lines */\n",
        "    public function test(array $lines): void {\n",
        "        foreach ($lines as $key => $line) {\n",
        "            if (!$line) {\n",
        "                continue;\n",
        "            }\n",
        "            $line->\n",
        "        }\n",
        "    }\n",
        "}\n",
    );

    open_php(&backend, &uri, text).await;

    let result = completion_response(&backend, &uri, 11, 19, None).await;

    assert!(result.is_some(), "Should return completions");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let prop_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::PROPERTY))
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                prop_names.contains(&"actualAmount"),
                "Should include 'actualAmount' after falsy guard with continue, got: {:?}",
                prop_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// `if ($var === null) { return; }` should narrow `$var` to non-null.
#[tokio::test]
async fn test_guard_clause_null_identity_return_narrows() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///guard_null_identity.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Formatter {\n",
        "    public function format(string $s): string { return $s; }\n",
        "}\n",
        "class Svc {\n",
        "    public function test(?Formatter $fmt): void {\n",
        "        if ($fmt === null) {\n",
        "            return;\n",
        "        }\n",
        "        $fmt->\n",
        "    }\n",
        "}\n",
    );

    open_php(&backend, &uri, text).await;

    let result = completion_response(&backend, &uri, 9, 14, None).await;

    assert!(result.is_some(), "Should return completions");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"format"),
                "Should include 'format' after null identity guard, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// `if (null === $var) { return; }` — reversed operand order should also work.
#[tokio::test]
async fn test_guard_clause_null_identity_reversed_operands() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///guard_null_reversed.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Logger {\n",
        "    public function info(string $msg): void {}\n",
        "}\n",
        "class Svc {\n",
        "    public function test(?Logger $log): void {\n",
        "        if (null === $log) {\n",
        "            return;\n",
        "        }\n",
        "        $log->\n",
        "    }\n",
        "}\n",
    );

    open_php(&backend, &uri, text).await;

    let result = completion_response(&backend, &uri, 9, 14, None).await;

    assert!(result.is_some(), "Should return completions");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"info"),
                "Should include 'info' after reversed null identity guard, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// `if ($var == null) { return; }` — equality (not identity) should also narrow.
#[tokio::test]
async fn test_guard_clause_null_equality_return_narrows() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///guard_null_eq.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Config {\n",
        "    public function get(string $key): string { return ''; }\n",
        "}\n",
        "class Svc {\n",
        "    public function test(?Config $cfg): void {\n",
        "        if ($cfg == null) {\n",
        "            return;\n",
        "        }\n",
        "        $cfg->\n",
        "    }\n",
        "}\n",
    );

    open_php(&backend, &uri, text).await;

    let result = completion_response(&backend, &uri, 9, 14, None).await;

    assert!(result.is_some(), "Should return completions");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"get"),
                "Should include 'get' after null equality guard, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Variable assigned via `?? null` then guarded with `!$var` + `continue`.
/// Reproduces the exact pattern from the null/falsy guard clause narrowing bug.
#[tokio::test]
async fn test_guard_clause_property_negated_instanceof_narrows() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///guard_prop_neg_instanceof.php").unwrap();
    // Faithfully reproduces the PropertyNarrowingDemo from example.php:
    // - Namespace wrapping
    // - A preceding positive instanceof if-block (non-guard)
    // - A negated instanceof guard clause
    // - Cursor after the guard clause
    let text = concat!(
        "<?php\n",
        "namespace Demo;\n",
        "class Dog {\n",
        "    public function bark(): void {}\n",
        "}\n",
        "class Cat {\n",
        "    public function purr(): void {}\n",
        "}\n",
        "class Svc {\n",
        "    private Dog|Cat $pet;\n",
        "    public function test(): void {\n",
        "        if ($this->pet instanceof Cat) {\n",
        "            $this->pet->purr();\n",
        "        }\n",
        "        if (!$this->pet instanceof Dog) {\n",
        "            return;\n",
        "        }\n",
        "        $this->pet->\n",
        "    }\n",
        "}\n",
    );

    open_php(&backend, &uri, text).await;

    let result = completion_response(&backend, &uri, 17, 22, None).await;

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
                "Should include Dog's 'bark' after negated instanceof guard on property, got: {:?}",
                method_names
            );
            assert!(
                !method_names.contains(&"purr"),
                "Should NOT include Cat's 'purr' after negated instanceof guard on property, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_guard_clause_property_positive_instanceof_excludes() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///guard_prop_pos_instanceof.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Dog {\n",
        "    public function bark(): void {}\n",
        "}\n",
        "class Cat {\n",
        "    public function purr(): void {}\n",
        "}\n",
        "class Svc {\n",
        "    private Dog|Cat $pet;\n",
        "    public function test(): void {\n",
        "        if ($this->pet instanceof Cat) {\n",
        "            return;\n",
        "        }\n",
        "        $this->pet->\n",
        "    }\n",
        "}\n",
    );

    open_php(&backend, &uri, text).await;

    let result = completion_response(&backend, &uri, 13, 22, None).await;

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
                "Should include Dog's 'bark' after positive instanceof guard excludes Cat, got: {:?}",
                method_names
            );
            assert!(
                !method_names.contains(&"purr"),
                "Should NOT include Cat's 'purr' after positive instanceof guard excludes Cat, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_guard_clause_null_coalesce_then_falsy_continue() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///guard_coalesce_continue.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class WarehouseOrderLine {\n",
        "    public int $actualAmount;\n",
        "    public int $amount;\n",
        "}\n",
        "class Svc {\n",
        "    /** @param array<int, WarehouseOrderLine> $warehouseOrderLines */\n",
        "    public function test(array $warehouseOrderLines): void {\n",
        "        foreach ($warehouseOrderLines as $key => $val) {\n",
        "            $warehouseOrderline = $warehouseOrderLines[$key] ?? null;\n",
        "            if (!$warehouseOrderline) {\n",
        "                continue;\n",
        "            }\n",
        "            $warehouseOrderline->\n",
        "        }\n",
        "    }\n",
        "}\n",
    );

    open_php(&backend, &uri, text).await;

    let result = completion_response(&backend, &uri, 13, 33, None).await;

    assert!(result.is_some(), "Should return completions");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let prop_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::PROPERTY))
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                prop_names.contains(&"actualAmount"),
                "Should include 'actualAmount' after null coalesce + falsy guard, got: {:?}",
                prop_names
            );
            assert!(
                prop_names.contains(&"amount"),
                "Should include 'amount' after null coalesce + falsy guard, got: {:?}",
                prop_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_guard_clause_instanceof_interface_narrows() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///guard_instanceof_interface.php").unwrap();
    let text = concat!(
        "<?php\n",
        "interface Identifiable {\n",
        "    public function getId(): int;\n",
        "}\n",
        "interface Nameable {\n",
        "    public function getName(): string;\n",
        "}\n",
        "class Svc {\n",
        "    public function test(object $obj): void {\n",
        "        if (!$obj instanceof Identifiable) {\n",
        "            return;\n",
        "        }\n",
        "        $obj->\n",
        "    }\n",
        "}\n",
    );

    open_php(&backend, &uri, text).await;

    let result = completion_response(&backend, &uri, 12, 14, None).await;

    assert!(result.is_some(), "Should return completions");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"getId"),
                "Should include 'getId' after negated instanceof guard, got: {:?}",
                method_names
            );
            assert!(
                !method_names.contains(&"getName"),
                "Should NOT include 'getName' (not narrowed to Nameable), got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_guard_clause_instanceof_interface_positive_branch() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///guard_instanceof_iface_positive.php").unwrap();
    let text = concat!(
        "<?php\n",
        "interface Identifiable {\n",
        "    public function getId(): int;\n",
        "}\n",
        "interface Nameable {\n",
        "    public function getName(): string;\n",
        "}\n",
        "class Svc {\n",
        "    public function test(object $obj): void {\n",
        "        if ($obj instanceof Nameable) {\n",
        "            $obj->\n",
        "        }\n",
        "    }\n",
        "}\n",
    );

    open_php(&backend, &uri, text).await;

    let result = completion_response(&backend, &uri, 10, 18, None).await;

    assert!(result.is_some(), "Should return completions");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"getName"),
                "Should include 'getName' inside positive instanceof branch, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_guard_clause_sequential_instanceof_interfaces() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///guard_sequential_instanceof.php").unwrap();
    let text = concat!(
        "<?php\n",
        "interface Identifiable {\n",
        "    public function getId(): int;\n",
        "}\n",
        "interface Nameable {\n",
        "    public function getName(): string;\n",
        "}\n",
        "class Svc {\n",
        "    public function test(object $obj): void {\n",
        "        if (!$obj instanceof Identifiable) {\n",
        "            return;\n",
        "        }\n",
        "        if (!$obj instanceof Nameable) {\n",
        "            return;\n",
        "        }\n",
        "        $obj->\n",
        "    }\n",
        "}\n",
    );

    open_php(&backend, &uri, text).await;

    let result = completion_response(&backend, &uri, 15, 14, None).await;

    assert!(result.is_some(), "Should return completions");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"getId"),
                "Should include 'getId' after sequential instanceof guards, got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"getName"),
                "Should include 'getName' after sequential instanceof guards, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_switch_default_throw_narrows_variable() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///switch_default_throw.php").unwrap();
    let text = concat!(
        "<?php\n",                                         // 0
        "class Dog { public function bark(): void {} }\n", // 1
        "class Cat { public function meow(): void {} }\n", // 2
        "/** @var string $type */\n",                      // 3
        "$type = 'dog';\n",                                // 4
        "/** @var Dog|Cat|null $pet */\n",                 // 5
        "$pet = null;\n",                                  // 6
        "switch ($type) {\n",                              // 7
        "    case 'dog':\n",                               // 8
        "        $pet = new Dog();\n",                     // 9
        "        break;\n",                                // 10
        "    case 'cat':\n",                               // 11
        "        $pet = new Cat();\n",                     // 12
        "        break;\n",                                // 13
        "    default:\n",                                  // 14
        "        throw new \\Exception('unknown');\n",     // 15
        "}\n",                                             // 16
        "$pet->\n",                                        // 17
    );

    open_php(&backend, &uri, text).await;

    let result = completion_response(&backend, &uri, 17, 6, None).await;

    assert!(result.is_some(), "Should return completions");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"bark"),
                "Should include Dog's 'bark' after switch with default throw, got: {:?}",
                method_names
            );
            assert!(
                method_names.contains(&"meow"),
                "Should include Cat's 'meow' after switch with default throw, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

#[tokio::test]
async fn test_guard_clause_colon_delimited_negated_instanceof_return_narrows() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///guard_neg_instanceof_colon.php").unwrap();
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
        "        if (!$pet instanceof Dog):\n",
        "            return;\n",
        "        endif;\n",
        "        $pet->\n",
        "    }\n",
        "}\n",
    );

    open_php(&backend, &uri, text).await;

    let result = completion_response(&backend, &uri, 13, 14, None).await;

    assert!(result.is_some(), "Should return completions");
    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let method_names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();

            assert!(
                method_names.contains(&"bark"),
                "Should include Dog's method 'bark' after colon-delimited guard clause, got: {:?}",
                method_names
            );
            assert!(
                !method_names.contains(&"purr"),
                "Should NOT include Cat's method 'purr' after colon-delimited guard clause, got: {:?}",
                method_names
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}
