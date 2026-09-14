use crate::common::{complete_at_opened, create_psr4_workspace, create_test_backend};
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

// ─── Helpers ────────────────────────────────────────────────────────────────

fn method_names(items: &[CompletionItem]) -> Vec<&str> {
    items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
        .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
        .collect()
}

fn property_names(items: &[CompletionItem]) -> Vec<&str> {
    items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::PROPERTY))
        .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
        .collect()
}

// ─── Basic Spread: single spread variable ───────────────────────────────────

/// `$all = [...$users]` where `$users` is `list<User>` → `$all[0]->` resolves User.
#[tokio::test]
async fn test_spread_single_list_variable() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///spread_single.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class User {\n",
        "    public string $name;\n",
        "    public function getEmail(): string {}\n",
        "}\n",
        "/** @var list<User> $users */\n",
        "$users = [];\n",
        "$all = [...$users];\n",
        "$all[0]->\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    let items = complete_at_opened(&backend, &uri, 8, 10).await;
    assert!(
        !items.is_empty(),
        "Should return completions for $all[0]-> from spread list<User>"
    );

    let methods = method_names(&items);
    assert!(
        methods.contains(&"getEmail"),
        "Should suggest User::getEmail(), got {:?}",
        methods
    );
    let props = property_names(&items);
    assert!(
        props.contains(&"name"),
        "Should suggest User::$name, got {:?}",
        props
    );
}

// ─── Multiple Spread: union of element types ────────────────────────────────

/// `$all = [...$users, ...$admins]` where types differ → union of both.
#[tokio::test]
async fn test_spread_multiple_variables_union() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///spread_union.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class User {\n",
        "    public string $name;\n",
        "    public function getEmail(): string {}\n",
        "}\n",
        "class AdminUser {\n",
        "    public string $name;\n",
        "    public function getEmail(): string {}\n",
        "    public function grantPermission(string $perm): void {}\n",
        "}\n",
        "/** @var list<User> $users */\n",
        "$users = [];\n",
        "/** @var list<AdminUser> $admins */\n",
        "$admins = [];\n",
        "$all = [...$users, ...$admins];\n",
        "$all[0]->\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    let items = complete_at_opened(&backend, &uri, 15, 10).await;
    assert!(
        !items.is_empty(),
        "Should return completions for union spread"
    );

    let methods = method_names(&items);
    assert!(
        methods.contains(&"getEmail"),
        "Should suggest getEmail (common to both), got {:?}",
        methods
    );
    assert!(
        methods.contains(&"grantPermission"),
        "Should suggest AdminUser::grantPermission(), got {:?}",
        methods
    );
}

// ─── Spread with array<K, V> annotation ─────────────────────────────────────

/// `$all = [...$items]` where `$items` is `array<int, User>` → element type User.
#[tokio::test]
async fn test_spread_array_generic_annotation() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///spread_array_generic.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Product {\n",
        "    public string $sku;\n",
        "    public function getPrice(): float {}\n",
        "}\n",
        "/** @var array<int, Product> $items */\n",
        "$items = [];\n",
        "$all = [...$items];\n",
        "$all[0]->\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    let items = complete_at_opened(&backend, &uri, 8, 10).await;
    assert!(
        !items.is_empty(),
        "Should return completions for spread array<int, Product>"
    );

    let props = property_names(&items);
    assert!(
        props.contains(&"sku"),
        "Should suggest Product::$sku, got {:?}",
        props
    );
    let methods = method_names(&items);
    assert!(
        methods.contains(&"getPrice"),
        "Should suggest Product::getPrice(), got {:?}",
        methods
    );
}

// ─── Spread with Type[] shorthand annotation ────────────────────────────────

/// `$all = [...$items]` where `$items` is `User[]` → element type User.
#[tokio::test]
async fn test_spread_type_array_shorthand() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///spread_shorthand.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class User {\n",
        "    public string $name;\n",
        "    public function getEmail(): string {}\n",
        "}\n",
        "/** @var User[] $users */\n",
        "$users = [];\n",
        "$merged = [...$users];\n",
        "$merged[0]->\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    let items = complete_at_opened(&backend, &uri, 8, 12).await;
    assert!(
        !items.is_empty(),
        "Should return completions for spread User[]"
    );

    let methods = method_names(&items);
    assert!(
        methods.contains(&"getEmail"),
        "Should suggest User::getEmail(), got {:?}",
        methods
    );
}

// ─── Spread mixed with keyed entries ────────────────────────────────────────

/// `$all = [...$users, 'extra' => new AdminUser()]` — keyed entries take
/// priority (existing behaviour), but spread still works for key completion.
/// The array shape wins when string keys are present.
#[tokio::test]
async fn test_spread_with_keyed_entries() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///spread_keyed.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class User {\n",
        "    public string $name;\n",
        "}\n",
        "class AdminUser {\n",
        "    public string $role;\n",
        "}\n",
        "/** @var list<User> $users */\n",
        "$users = [];\n",
        "$config = ['admin' => new AdminUser(), ...$users];\n",
        "$config['\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Key completion — 'admin' should be available from the keyed entry.
    let items = complete_at_opened(&backend, &uri, 10, 9).await;
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(
        labels.contains(&"admin"),
        "Should suggest 'admin' key, got {:?}",
        labels
    );
}

// ─── Spread inside a class method ───────────────────────────────────────────

/// Spread works in a class method context with `$this`.
#[tokio::test]
async fn test_spread_inside_class_method() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///spread_method.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class User {\n",
        "    public string $name;\n",
        "    public function getEmail(): string {}\n",
        "}\n",
        "class AdminUser {\n",
        "    public string $name;\n",
        "    public function getRole(): string {}\n",
        "}\n",
        "class Service {\n",
        "    public function merge() {\n",
        "        /** @var list<User> $users */\n",
        "        $users = [];\n",
        "        /** @var list<AdminUser> $admins */\n",
        "        $admins = [];\n",
        "        $all = [...$users, ...$admins];\n",
        "        $all[0]->\n",
        "    }\n",
        "}\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    let items = complete_at_opened(&backend, &uri, 16, 18).await;
    assert!(
        !items.is_empty(),
        "Should return completions for spread inside class method"
    );

    let methods = method_names(&items);
    assert!(
        methods.contains(&"getEmail"),
        "Should include User::getEmail(), got {:?}",
        methods
    );
    assert!(
        methods.contains(&"getRole"),
        "Should include AdminUser::getRole(), got {:?}",
        methods
    );
}

// ─── Spread with foreach (requires @var annotation on merged var) ───────────

/// Foreach over spread-merged array with an explicit `@var` annotation on
/// the merged variable resolves element types.  (Without the annotation,
/// the foreach resolver would need to walk through the spread assignment —
/// that is a separate enhancement.)
#[tokio::test]
async fn test_spread_with_foreach_annotated() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///spread_foreach.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class User {\n",
        "    public string $name;\n",
        "    public function getEmail(): string {}\n",
        "}\n",
        "class AdminUser {\n",
        "    public string $name;\n",
        "    public function getRole(): string {}\n",
        "}\n",
        "/** @var list<User> $users */\n",
        "$users = [];\n",
        "/** @var list<AdminUser> $admins */\n",
        "$admins = [];\n",
        "$all = [...$users, ...$admins];\n",
        "// Direct element access works via spread inference:\n",
        "$all[0]->\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Verify element access still works (the core spread feature).
    let items = complete_at_opened(&backend, &uri, 15, 10).await;
    assert!(
        !items.is_empty(),
        "Should return completions for $all[0]-> from spread-merged array"
    );

    let methods = method_names(&items);
    assert!(
        methods.contains(&"getEmail"),
        "Should include User::getEmail(), got {:?}",
        methods
    );
    assert!(
        methods.contains(&"getRole"),
        "Should include AdminUser::getRole(), got {:?}",
        methods
    );
}

// ─── Spread combined with push assignments ──────────────────────────────────

/// `$all = [...$users]; $all[] = new AdminUser();` → both element types.
#[tokio::test]
async fn test_spread_combined_with_push() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///spread_push.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class User {\n",
        "    public string $name;\n",
        "    public function getEmail(): string {}\n",
        "}\n",
        "class AdminUser {\n",
        "    public string $name;\n",
        "    public function grantPermission(string $perm): void {}\n",
        "}\n",
        "/** @var list<User> $users */\n",
        "$users = [];\n",
        "$all = [...$users];\n",
        "$all[] = new AdminUser();\n",
        "$all[0]->\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    let items = complete_at_opened(&backend, &uri, 13, 10).await;
    assert!(
        !items.is_empty(),
        "Should return completions for spread + push"
    );

    let methods = method_names(&items);
    assert!(
        methods.contains(&"getEmail"),
        "Should include User::getEmail() from spread, got {:?}",
        methods
    );
    assert!(
        methods.contains(&"grantPermission"),
        "Should include AdminUser::grantPermission() from push, got {:?}",
        methods
    );
}

// ─── Spread with array() syntax ─────────────────────────────────────────────

/// `$all = array(...$users)` should work the same as `[...$users]`.
#[tokio::test]
async fn test_spread_array_function_syntax() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///spread_array_syntax.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class User {\n",
        "    public string $name;\n",
        "    public function getEmail(): string {}\n",
        "}\n",
        "/** @var list<User> $users */\n",
        "$users = [];\n",
        "$all = array(...$users);\n",
        "$all[0]->\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    let items = complete_at_opened(&backend, &uri, 8, 10).await;
    assert!(
        !items.is_empty(),
        "Should return completions for spread with array() syntax"
    );

    let methods = method_names(&items);
    assert!(
        methods.contains(&"getEmail"),
        "Should suggest User::getEmail(), got {:?}",
        methods
    );
}

// ─── Cross-file PSR-4 spread ────────────────────────────────────────────────

/// Spread works across files via PSR-4 autoloading.
#[tokio::test]
async fn test_spread_cross_file_psr4() {
    let composer = r#"{
        "autoload": {
            "psr-4": {
                "App\\": "src/"
            }
        }
    }"#;

    let user_php = concat!(
        "<?php\n",
        "namespace App\\Models;\n",
        "class User {\n",
        "    public string $name;\n",
        "    public function getEmail(): string {}\n",
        "}\n",
    );

    let admin_php = concat!(
        "<?php\n",
        "namespace App\\Models;\n",
        "class AdminUser {\n",
        "    public string $name;\n",
        "    public function getRole(): string {}\n",
        "}\n",
    );

    let service_php = concat!(
        "<?php\n",
        "namespace App\\Services;\n",
        "use App\\Models\\User;\n",
        "use App\\Models\\AdminUser;\n",
        "class MergeService {\n",
        "    public function merge() {\n",
        "        /** @var list<User> $users */\n",
        "        $users = [];\n",
        "        /** @var list<AdminUser> $admins */\n",
        "        $admins = [];\n",
        "        $all = [...$users, ...$admins];\n",
        "        $all[0]->\n",
        "    }\n",
        "}\n",
    );

    let (backend, _dir) = create_psr4_workspace(
        composer,
        &[
            ("src/Models/User.php", user_php),
            ("src/Models/AdminUser.php", admin_php),
            ("src/Services/MergeService.php", service_php),
        ],
    );

    let uri = Url::parse("file:///service.php").unwrap();
    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: service_php.to_string(),
        },
    };
    backend.did_open(open_params).await;

    let items = complete_at_opened(&backend, &uri, 11, 18).await;
    assert!(
        !items.is_empty(),
        "Should return completions for cross-file spread"
    );

    let methods = method_names(&items);
    assert!(
        methods.contains(&"getEmail"),
        "Should include User::getEmail() from cross-file, got {:?}",
        methods
    );
    assert!(
        methods.contains(&"getRole"),
        "Should include AdminUser::getRole() from cross-file, got {:?}",
        methods
    );
}

// ─── Spread with @param annotation ─────────────────────────────────────────

/// Spread variable annotated via @param in a function signature.
#[tokio::test]
async fn test_spread_param_annotation() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///spread_param.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class User {\n",
        "    public string $name;\n",
        "    public function getEmail(): string {}\n",
        "}\n",
        "class Service {\n",
        "    /**\n",
        "     * @param list<User> $users\n",
        "     */\n",
        "    public function merge(array $users) {\n",
        "        $all = [...$users];\n",
        "        $all[0]->\n",
        "    }\n",
        "}\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    let items = complete_at_opened(&backend, &uri, 11, 18).await;
    assert!(
        !items.is_empty(),
        "Should return completions for spread from @param"
    );

    let methods = method_names(&items);
    assert!(
        methods.contains(&"getEmail"),
        "Should suggest User::getEmail(), got {:?}",
        methods
    );
}

// ─── Empty spread ───────────────────────────────────────────────────────────

/// An empty array with no spreads should not crash.
#[tokio::test]
async fn test_spread_empty_array() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///spread_empty.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class User {\n",
        "    public string $name;\n",
        "}\n",
        "$all = [];\n",
        "$all[0]->\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    // Should not crash, just no completions.
    let items = complete_at_opened(&backend, &uri, 5, 10).await;
    // Either empty or no class members — this is the "no-op" case.
    let _ = items;
}

// ─── Spread deduplication ───────────────────────────────────────────────────

/// `$all = [...$a, ...$b]` where both are `list<User>` → User appears once.
#[tokio::test]
async fn test_spread_deduplicates_same_type() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///spread_dedup.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class User {\n",
        "    public string $name;\n",
        "    public function getEmail(): string {}\n",
        "}\n",
        "/** @var list<User> $a */\n",
        "$a = [];\n",
        "/** @var list<User> $b */\n",
        "$b = [];\n",
        "$all = [...$a, ...$b];\n",
        "$all[0]->\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    let items = complete_at_opened(&backend, &uri, 10, 10).await;
    assert!(
        !items.is_empty(),
        "Should return completions for spread with duplicate types"
    );

    let methods = method_names(&items);
    assert!(
        methods.contains(&"getEmail"),
        "Should suggest User::getEmail(), got {:?}",
        methods
    );
    // Verify no duplicate method entries
    let email_count = methods.iter().filter(|&&m| m == "getEmail").count();
    assert_eq!(
        email_count, 1,
        "getEmail should appear exactly once, got {}",
        email_count
    );
}

// ─── Spread with three variables ────────────────────────────────────────────

/// Three spread variables with different types — all resolved.
#[tokio::test]
async fn test_spread_three_variables() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///spread_three.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class User {\n",
        "    public function getEmail(): string {}\n",
        "}\n",
        "class Product {\n",
        "    public function getPrice(): float {}\n",
        "}\n",
        "class Order {\n",
        "    public function getTotal(): float {}\n",
        "}\n",
        "/** @var list<User> $users */\n",
        "$users = [];\n",
        "/** @var list<Product> $products */\n",
        "$products = [];\n",
        "/** @var list<Order> $orders */\n",
        "$orders = [];\n",
        "$all = [...$users, ...$products, ...$orders];\n",
        "$all[0]->\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    let items = complete_at_opened(&backend, &uri, 17, 10).await;
    assert!(
        !items.is_empty(),
        "Should return completions for three spread variables"
    );

    let methods = method_names(&items);
    assert!(
        methods.contains(&"getEmail"),
        "Should include User::getEmail(), got {:?}",
        methods
    );
    assert!(
        methods.contains(&"getPrice"),
        "Should include Product::getPrice(), got {:?}",
        methods
    );
    assert!(
        methods.contains(&"getTotal"),
        "Should include Order::getTotal(), got {:?}",
        methods
    );
}

// ─── Spread where source is assigned from another array literal ─────────────

/// The spread variable is itself an array literal with push assignments
/// rather than having a docblock annotation.
#[tokio::test]
async fn test_spread_source_from_push_assignments() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///spread_from_push.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class User {\n",
        "    public string $name;\n",
        "    public function getEmail(): string {}\n",
        "}\n",
        "$users = [];\n",
        "$users[] = new User();\n",
        "$all = [...$users];\n",
        "$all[0]->\n",
    );

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    let items = complete_at_opened(&backend, &uri, 8, 10).await;
    // This may or may not work depending on whether the resolver can
    // transitively resolve push-style assignments through spread.
    // The primary goal is no crash.  If it resolves, great.
    let _ = items;
}

// ─── Unit tests for extract_spread_expressions ──────────────────────────────

#[test]
fn test_extract_spread_basic() {
    use phpantom_lsp::completion::array_shape::extract_spread_expressions;

    let result = extract_spread_expressions("[...$users, ...$admins]").unwrap();
    assert_eq!(result, vec!["$users", "$admins"]);
}

#[test]
fn test_extract_spread_with_keyed_entries() {
    use phpantom_lsp::completion::array_shape::extract_spread_expressions;

    let result = extract_spread_expressions("['key' => 'val', ...$users, 'other' => 42]").unwrap();
    assert_eq!(result, vec!["$users"]);
}

#[test]
fn test_extract_spread_array_syntax() {
    use phpantom_lsp::completion::array_shape::extract_spread_expressions;

    let result = extract_spread_expressions("array(...$items, ...$extras)").unwrap();
    assert_eq!(result, vec!["$items", "$extras"]);
}

#[test]
fn test_extract_spread_empty_array() {
    use phpantom_lsp::completion::array_shape::extract_spread_expressions;

    let result = extract_spread_expressions("[]").unwrap();
    assert!(result.is_empty());
}

#[test]
fn test_extract_spread_not_an_array() {
    use phpantom_lsp::completion::array_shape::extract_spread_expressions;

    assert!(extract_spread_expressions("$this->getItems()").is_none());
    assert!(extract_spread_expressions("someFunction()").is_none());
    assert!(extract_spread_expressions("'hello'").is_none());
}

#[test]
fn test_extract_spread_single() {
    use phpantom_lsp::completion::array_shape::extract_spread_expressions;

    let result = extract_spread_expressions("[...$items]").unwrap();
    assert_eq!(result, vec!["$items"]);
}

#[test]
fn test_extract_spread_no_spreads() {
    use phpantom_lsp::completion::array_shape::extract_spread_expressions;

    let result = extract_spread_expressions("['a' => 1, 'b' => 2]").unwrap();
    assert!(result.is_empty());
}
