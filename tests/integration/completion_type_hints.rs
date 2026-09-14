use crate::common::{class_items, complete_at, create_test_backend, labels};
use tower_lsp::lsp_types::*;

// ─── Helper ─────────────────────────────────────────────────────────────────

/// Filter to only KEYWORD-kind items (native types).
fn keyword_items(items: &[CompletionItem]) -> Vec<&CompletionItem> {
    items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::KEYWORD))
        .collect()
}

// ─── Function parameter type hints ──────────────────────────────────────────

#[tokio::test]
async fn param_type_offers_scalar_types() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/param_scalar.php").unwrap();

    // Cursor after `(str` — should offer `string`
    let src = "<?php\nfunction foo(str) {}";
    // `str` ends at col 15 (s=13, t=14, r=15, )=16 → cursor at 16? No.
    // f=0 u=1 n=2 c=3 t=4 i=5 o=6 n=7 ' '=8 f=9 o=10 o=11 (=12 s=13 t=14 r=15 )=16
    // cursor after "str" = col 16? Actually col 16 is ')'.
    // We want cursor at col 16 which is where ')' is, partial = "str"
    let items = complete_at(&backend, &uri, src, 1, 16).await;
    let kw = keyword_items(&items);
    let kw_labels: Vec<&str> = kw.iter().map(|i| i.label.as_str()).collect();
    assert!(
        kw_labels.contains(&"string"),
        "expected 'string' in {:?}",
        kw_labels
    );
}

#[tokio::test]
async fn param_type_offers_all_native_types_on_empty() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/param_all.php").unwrap();

    // Cursor right after `(` with nothing typed
    let src = "<?php\nfunction foo() {}";
    // (=12 )=13 → cursor at 13
    let items = complete_at(&backend, &uri, src, 1, 13).await;
    let kw = keyword_items(&items);
    let kw_labels: Vec<&str> = kw.iter().map(|i| i.label.as_str()).collect();

    // Should include common PHP native types
    for expected in &[
        "string", "int", "float", "bool", "array", "mixed", "callable",
    ] {
        assert!(
            kw_labels.contains(expected),
            "expected '{}' in {:?}",
            expected,
            kw_labels
        );
    }
}

#[tokio::test]
async fn param_type_offers_class_names() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/param_class.php").unwrap();

    let src = r#"<?php
class UserService {}
class UserRepository {}

function handleUser(User) {}
"#;
    // Line 4: `function handleUser(User) {}`
    // f=0 ... (=19 U=20 s=21 e=22 r=23 )=24
    // cursor after "User" = col 24
    let items = complete_at(&backend, &uri, src, 4, 24).await;
    let cls = class_items(&items);
    let cls_labels: Vec<&str> = cls.iter().map(|i| i.label.as_str()).collect();

    assert!(
        cls_labels.contains(&"UserService"),
        "expected 'UserService' in {:?}",
        cls_labels
    );
    assert!(
        cls_labels.contains(&"UserRepository"),
        "expected 'UserRepository' in {:?}",
        cls_labels
    );
}

#[tokio::test]
async fn param_type_does_not_offer_constants_or_functions() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/param_no_const.php").unwrap();

    let src = r#"<?php
function myHelper() {}

function foo(my) {}
"#;
    // Line 3: `function foo(my) {}`
    // (=12 m=13 y=14 )=15
    // cursor after "my" = col 15
    let items = complete_at(&backend, &uri, src, 3, 15).await;
    let func_items: Vec<&CompletionItem> = items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::FUNCTION))
        .collect();

    // No function completions should appear in a type-hint position
    assert!(
        func_items.is_empty(),
        "expected no FUNCTION items but got: {:?}",
        func_items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn param_type_after_comma() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/param_comma.php").unwrap();

    let src = "<?php\nfunction foo(string $a, int) {}";
    // Line 1: function foo(string $a, int) {}
    // s=13 t=14 r=15 i=16 n=17 g=18 ' '=19 $=20 a=21 ,=22 ' '=23 i=24 n=25 t=26 )=27
    // cursor after "int" = col 27
    let items = complete_at(&backend, &uri, src, 1, 27).await;
    let kw = keyword_items(&items);
    let kw_labels: Vec<&str> = kw.iter().map(|i| i.label.as_str()).collect();
    assert!(
        kw_labels.contains(&"int"),
        "expected 'int' in {:?}",
        kw_labels
    );
}

// ─── Return type hints ─────────────────────────────────────────────────────

#[tokio::test]
async fn return_type_offers_scalar_types() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/return_scalar.php").unwrap();

    let src = "<?php\nfunction foo(): voi {}";
    // Line 1: function foo(): voi {}
    // )=13 :=14 ' '=15 v=16 o=17 i=18 ' '=19
    // cursor after "voi" = col 19
    let items = complete_at(&backend, &uri, src, 1, 19).await;
    let kw = keyword_items(&items);
    let kw_labels: Vec<&str> = kw.iter().map(|i| i.label.as_str()).collect();
    assert!(
        kw_labels.contains(&"void"),
        "expected 'void' in {:?}",
        kw_labels
    );
}

#[tokio::test]
async fn return_type_offers_class_names() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/return_class.php").unwrap();

    let src = r#"<?php
class Response {}

function handle(): Resp {}
"#;
    // Line 3: `function handle(): Resp {}`
    // )=16 :=17 ' '=18 R=19 e=20 s=21 p=22 ' '=23
    // cursor after "Resp" = col 23
    let items = complete_at(&backend, &uri, src, 3, 23).await;
    let cls = class_items(&items);
    let cls_labels: Vec<&str> = cls.iter().map(|i| i.label.as_str()).collect();

    assert!(
        cls_labels.contains(&"Response"),
        "expected 'Response' in {:?}",
        cls_labels
    );
}

// ─── Nullable / union / intersection types ──────────────────────────────────

#[tokio::test]
async fn nullable_param_type() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/nullable.php").unwrap();

    let src = "<?php\nfunction foo(?str) {}";
    // (=12 ?=13 s=14 t=15 r=16 )=17
    // cursor after "str" = col 17
    let items = complete_at(&backend, &uri, src, 1, 17).await;
    let kw = keyword_items(&items);
    let kw_labels: Vec<&str> = kw.iter().map(|i| i.label.as_str()).collect();
    assert!(
        kw_labels.contains(&"string"),
        "expected 'string' in {:?}",
        kw_labels
    );
}

#[tokio::test]
async fn union_param_type() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/union.php").unwrap();

    let src = "<?php\nfunction foo(string|int) {}";
    // (=12 s=13...g=18 |=19 i=20 n=21 t=22 )=23
    // cursor after "int" = col 23
    let items = complete_at(&backend, &uri, src, 1, 23).await;
    let kw = keyword_items(&items);
    let kw_labels: Vec<&str> = kw.iter().map(|i| i.label.as_str()).collect();
    assert!(
        kw_labels.contains(&"int"),
        "expected 'int' in {:?}",
        kw_labels
    );
}

#[tokio::test]
async fn intersection_param_type() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/intersection.php").unwrap();

    let src = r#"<?php
interface Countable {}
interface Traversable {}

function foo(Countable&Trav) {}
"#;
    // Line 4: function foo(Countable&Trav) {}
    // (=12 C=13...e=21 &=22 T=23 r=24 a=25 v=26 )=27
    // cursor after "Trav" = col 27
    let items = complete_at(&backend, &uri, src, 4, 27).await;
    let cls = class_items(&items);
    let cls_labels: Vec<&str> = cls.iter().map(|i| i.label.as_str()).collect();

    assert!(
        cls_labels.contains(&"Traversable"),
        "expected 'Traversable' in {:?}",
        cls_labels
    );
}

#[tokio::test]
async fn union_return_type() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/union_return.php").unwrap();

    let src = "<?php\nfunction foo(): string|int {}";
    // )=13 :=14 ' '=15 s=16...g=21 |=22 i=23 n=24 t=25 ' '=26
    // cursor after "int" = col 26
    let items = complete_at(&backend, &uri, src, 1, 26).await;
    let kw = keyword_items(&items);
    let kw_labels: Vec<&str> = kw.iter().map(|i| i.label.as_str()).collect();
    assert!(
        kw_labels.contains(&"int"),
        "expected 'int' in {:?}",
        kw_labels
    );
}

// ─── Method definitions ─────────────────────────────────────────────────────

#[tokio::test]
async fn method_param_type() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/method_param.php").unwrap();

    let src = r#"<?php
class Foo {
    public function bar(str) {}
}
"#;
    // Line 2: `    public function bar(str) {}`
    // (=23 s=24 t=25 r=26 )=27
    // cursor after "str" = col 27
    let items = complete_at(&backend, &uri, src, 2, 27).await;
    let kw = keyword_items(&items);
    let kw_labels: Vec<&str> = kw.iter().map(|i| i.label.as_str()).collect();
    assert!(
        kw_labels.contains(&"string"),
        "expected 'string' in {:?}",
        kw_labels
    );
}

#[tokio::test]
async fn method_return_type() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/method_return.php").unwrap();

    let src = r#"<?php
class Foo {
    public function bar(): voi {}
}
"#;
    // Line 2: `    public function bar(): voi {}`
    // )=24 :=25 ' '=26 v=27 o=28 i=29 ' '=30
    // cursor after "voi" = col 30
    let items = complete_at(&backend, &uri, src, 2, 30).await;
    let kw = keyword_items(&items);
    let kw_labels: Vec<&str> = kw.iter().map(|i| i.label.as_str()).collect();
    assert!(
        kw_labels.contains(&"void"),
        "expected 'void' in {:?}",
        kw_labels
    );
}

// ─── Property type hints ───────────────────────────────────────────────────

#[tokio::test]
async fn property_type_after_public() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/prop_public.php").unwrap();

    let src = "<?php\nclass Foo {\n    public str\n}";
    // Line 2: `    public str`
    // p=4...c=9 ' '=10 s=11 t=12 r=13
    // cursor after "str" = col 14
    let items = complete_at(&backend, &uri, src, 2, 14).await;
    let kw = keyword_items(&items);
    let kw_labels: Vec<&str> = kw.iter().map(|i| i.label.as_str()).collect();
    assert!(
        kw_labels.contains(&"string"),
        "expected 'string' in {:?}",
        kw_labels
    );
}

#[tokio::test]
async fn property_type_after_private_readonly() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/prop_readonly.php").unwrap();

    let src = "<?php\nclass Foo {\n    private readonly str\n}";
    // Line 2: `    private readonly str`
    // p=4...e=10 ' '=11 r=12...y=19 ' '=20 s=21 t=22 r=23
    // cursor after "str" = col 24
    let items = complete_at(&backend, &uri, src, 2, 24).await;
    let kw = keyword_items(&items);
    let kw_labels: Vec<&str> = kw.iter().map(|i| i.label.as_str()).collect();
    assert!(
        kw_labels.contains(&"string"),
        "expected 'string' in {:?}",
        kw_labels
    );
}

// ─── Promoted constructor parameters ────────────────────────────────────────

#[tokio::test]
async fn promoted_param_type() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/promoted.php").unwrap();

    let src = r#"<?php
class Foo {
    public function __construct(private str) {}
}
"#;
    // Line 2: `    public function __construct(private str) {}`
    // (=31 p=32...e=38 ' '=39 s=40 t=41 r=42 )=43
    // cursor after "str" = col 43
    let items = complete_at(&backend, &uri, src, 2, 43).await;
    let kw = keyword_items(&items);
    let kw_labels: Vec<&str> = kw.iter().map(|i| i.label.as_str()).collect();
    assert!(
        kw_labels.contains(&"string"),
        "expected 'string' in {:?}",
        kw_labels
    );
}

#[tokio::test]
async fn promoted_param_with_class() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/promoted_class.php").unwrap();

    let src = r#"<?php
class UserService {}

class Controller {
    public function __construct(private User) {}
}
"#;
    // Line 4: `    public function __construct(private User) {}`
    // (=31 p=32...e=38 ' '=39 U=40 s=41 e=42 r=43 )=44
    // cursor after "User" = col 44
    let items = complete_at(&backend, &uri, src, 4, 44).await;
    let cls = class_items(&items);
    let cls_labels: Vec<&str> = cls.iter().map(|i| i.label.as_str()).collect();

    assert!(
        cls_labels.contains(&"UserService"),
        "expected 'UserService' in {:?}",
        cls_labels
    );
}

// ─── Closures and arrow functions ───────────────────────────────────────────

#[tokio::test]
async fn closure_param_type() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/closure.php").unwrap();

    let src = "<?php\n$f = function(str) {};";
    // Line 1: `$f = function(str) {};`
    // (=13 s=14 t=15 r=16 )=17
    // cursor after "str" = col 17
    let items = complete_at(&backend, &uri, src, 1, 17).await;
    let kw = keyword_items(&items);
    let kw_labels: Vec<&str> = kw.iter().map(|i| i.label.as_str()).collect();
    assert!(
        kw_labels.contains(&"string"),
        "expected 'string' in {:?}",
        kw_labels
    );
}

#[tokio::test]
async fn arrow_fn_param_type() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/arrow.php").unwrap();

    let src = "<?php\n$f = fn(str) => null;";
    // Line 1: `$f = fn(str) => null;`
    // (=7 s=8 t=9 r=10 )=11
    // cursor after "str" = col 11
    let items = complete_at(&backend, &uri, src, 1, 11).await;
    let kw = keyword_items(&items);
    let kw_labels: Vec<&str> = kw.iter().map(|i| i.label.as_str()).collect();
    assert!(
        kw_labels.contains(&"string"),
        "expected 'string' in {:?}",
        kw_labels
    );
}

#[tokio::test]
async fn closure_return_type() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/closure_ret.php").unwrap();

    let src = "<?php\n$f = function(): voi {};";
    // Line 1: `$f = function(): voi {};`
    // )=14 :=15 ' '=16 v=17 o=18 i=19 ' '=20
    // cursor after "voi" = col 20
    let items = complete_at(&backend, &uri, src, 1, 20).await;
    let kw = keyword_items(&items);
    let kw_labels: Vec<&str> = kw.iter().map(|i| i.label.as_str()).collect();
    assert!(
        kw_labels.contains(&"void"),
        "expected 'void' in {:?}",
        kw_labels
    );
}

// ─── Multi-line function definitions ────────────────────────────────────────

#[tokio::test]
async fn multiline_param_type() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/multiline.php").unwrap();

    let src = "<?php\nfunction foo(\n    string $a,\n    int\n) {}";
    // Line 3: `    int`
    // ' '=0-3 i=4 n=5 t=6
    // cursor after "int" = col 7
    let items = complete_at(&backend, &uri, src, 3, 7).await;
    let kw = keyword_items(&items);
    let kw_labels: Vec<&str> = kw.iter().map(|i| i.label.as_str()).collect();
    assert!(
        kw_labels.contains(&"int"),
        "expected 'int' in {:?}",
        kw_labels
    );
}

// ─── Negative cases: should NOT trigger type hint completion ────────────────

#[tokio::test]
async fn not_in_function_call() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/not_call.php").unwrap();

    // `str` inside a function CALL — should NOT get type-hint suggestions.
    // The class name completion might still fire, but the native-type keywords
    // with detail "PHP built-in type" should be absent.
    let src = "<?php\nfunction myFunc(string $s) {}\nmyFunc(str);";
    // Line 2: `myFunc(str);`
    // (=6 s=7 t=8 r=9 )=10
    // cursor after "str" = col 10
    let items = complete_at(&backend, &uri, src, 2, 10).await;
    let type_kws: Vec<&CompletionItem> = items
        .iter()
        .filter(|i| {
            i.kind == Some(CompletionItemKind::KEYWORD)
                && i.detail.as_deref() == Some("PHP built-in type")
        })
        .collect();

    assert!(
        type_kws.is_empty(),
        "type keywords should not appear in function calls, got: {:?}",
        type_kws.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn not_in_variable_position() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/not_var.php").unwrap();

    // `$str` — variable, not a type hint
    let src = "<?php\nfunction foo($str) {}";
    // $=12 s=13 t=14 r=15
    // cursor after "$str" = col 16
    let items = complete_at(&backend, &uri, src, 1, 16).await;
    let type_kws: Vec<&CompletionItem> = items
        .iter()
        .filter(|i| {
            i.kind == Some(CompletionItemKind::KEYWORD)
                && i.detail.as_deref() == Some("PHP built-in type")
        })
        .collect();

    assert!(
        type_kws.is_empty(),
        "type keywords should not appear in variable position, got: {:?}",
        type_kws.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
}

// ─── Type hint items have correct metadata ──────────────────────────────────

#[tokio::test]
async fn type_hint_items_have_correct_kind_and_detail() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/metadata.php").unwrap();

    let src = "<?php\nfunction foo(str) {}";
    // cursor after "str" = col 16
    let items = complete_at(&backend, &uri, src, 1, 16).await;

    let string_item = items.iter().find(|i| i.label == "string").unwrap();
    assert_eq!(string_item.kind, Some(CompletionItemKind::KEYWORD));
    assert_eq!(string_item.detail.as_deref(), Some("PHP built-in type"));
}

#[tokio::test]
async fn type_hint_filters_by_partial() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/filter.php").unwrap();

    // Typing `fl` — should match `float` but not `string`, `int`, etc.
    let src = "<?php\nfunction foo(fl) {}";
    // (=12 f=13 l=14 )=15
    // cursor after "fl" = col 15
    let items = complete_at(&backend, &uri, src, 1, 15).await;
    let kw = keyword_items(&items);
    let kw_labels: Vec<&str> = kw.iter().map(|i| i.label.as_str()).collect();

    assert!(
        kw_labels.contains(&"float"),
        "expected 'float' in {:?}",
        kw_labels
    );
    assert!(
        !kw_labels.contains(&"string"),
        "'string' should be filtered out"
    );
    assert!(!kw_labels.contains(&"int"), "'int' should be filtered out");
}

// ─── Cross-file class resolution in type hints ─────────────────────────────

#[tokio::test]
async fn type_hint_with_cross_file_class() {
    let (backend, _dir) = crate::common::create_psr4_workspace(
        r#"{ "autoload": { "psr-4": { "App\\": "src/" } } }"#,
        &[
            (
                "src/Models/User.php",
                "<?php\nnamespace App\\Models;\nclass User {\n    public function getName(): string { return ''; }\n}\n",
            ),
            (
                "src/Services/UserService.php",
                "<?php\nnamespace App\\Services;\nuse App\\Models\\User;\nclass UserService {\n    public function handle(User) {}\n}\n",
            ),
        ],
    );

    let uri = Url::parse("file:///test/cross_file.php").unwrap();
    let src = r#"<?php
namespace App\Services;
use App\Models\User;
class UserService {
    public function handle(Us) {}
}
"#;
    // Line 4: `    public function handle(Us) {}`
    // (=26 U=27 s=28 )=29
    // cursor after "Us" = col 29
    let items = complete_at(&backend, &uri, src, 4, 29).await;
    let all_labels = labels(&items);

    // Should include the User class from PSR-4
    assert!(
        all_labels.iter().any(|l| l.contains("User")),
        "expected a User-related class in {:?}",
        all_labels
    );
}

// ─── Completion with $this context ──────────────────────────────────────────

#[tokio::test]
async fn method_param_type_with_self() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/self.php").unwrap();

    // Typing `sel` in a param type position should offer `self`
    let src = r#"<?php
class Foo {
    public function bar(sel) {}
}
"#;
    // Line 2: `    public function bar(sel) {}`
    // (=23 s=24 e=25 l=26 )=27
    // cursor after "sel" = col 27
    let items = complete_at(&backend, &uri, src, 2, 27).await;
    let kw = keyword_items(&items);
    let kw_labels: Vec<&str> = kw.iter().map(|i| i.label.as_str()).collect();
    assert!(
        kw_labels.contains(&"self"),
        "expected 'self' in {:?}",
        kw_labels
    );
}

#[tokio::test]
async fn return_type_offers_static() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/static_ret.php").unwrap();

    let src = r#"<?php
class Foo {
    public function create(): stat {}
}
"#;
    // Line 2: `    public function create(): stat {}`
    // )=25 :=26 ' '=27 s=28 t=29 a=30 t=31 ' '=32
    // cursor after "stat" = col 32
    let items = complete_at(&backend, &uri, src, 2, 32).await;
    let kw = keyword_items(&items);
    let kw_labels: Vec<&str> = kw.iter().map(|i| i.label.as_str()).collect();
    assert!(
        kw_labels.contains(&"static"),
        "expected 'static' in {:?}",
        kw_labels
    );
}

// ─── Sort order: scalars before classes ─────────────────────────────────────

#[tokio::test]
async fn return_type_no_space_after_colon_inserts_space() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/return_space.php").unwrap();

    // The colon is present but there is NO space between `:` and the
    // partial type the user has started typing.
    let src_no_space = "<?php\nfunction foo():s {}";
    // )=13 :=14 s=15
    // cursor after "s" = col 15
    let items = complete_at(&backend, &uri, src_no_space, 1, 15).await;
    let string_item = items.iter().find(|i| i.label == "string");
    assert!(string_item.is_some(), "expected 'string' in results");
    let insert = string_item.unwrap().insert_text.as_deref().unwrap();
    assert!(
        insert.starts_with(' '),
        "insert_text should start with a space when colon has no trailing space, got: {:?}",
        insert
    );
    assert_eq!(
        insert.trim(),
        "string",
        "insert_text should contain 'string' after the space"
    );
}

/// When the colon already has a trailing space (`): s`), the inserted
/// text should NOT have an extra leading space.
#[tokio::test]
async fn return_type_with_space_after_colon_no_extra_space() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/return_no_extra_space.php").unwrap();

    let src = "<?php\nfunction foo(): s {}";
    // )=13 :=14 ' '=15 s=16
    // cursor after "s" = col 16
    let items = complete_at(&backend, &uri, src, 1, 16).await;
    let string_item = items.iter().find(|i| i.label == "string");
    assert!(string_item.is_some(), "expected 'string' in results");
    let insert = string_item.unwrap().insert_text.as_deref().unwrap();
    assert!(
        !insert.starts_with(' '),
        "insert_text should NOT start with a space when colon already has trailing space, got: {:?}",
        insert
    );
}

#[tokio::test]
async fn scalars_sort_before_classes() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/sort_order.php").unwrap();

    // Both `string` (scalar) and `StringHelper` (class) match the prefix "str".
    let src = r#"<?php
class StringHelper {}

function demo(str) {}
"#;
    // Line 3: `function demo(str) {}`
    // (=13 s=14 t=15 r=16 )=17
    // cursor after "str" = col 17
    let items = complete_at(&backend, &uri, src, 3, 17).await;

    // Find positions of `string` (scalar) and `StringHelper` (class).
    let string_pos = items.iter().position(|i| i.label == "string");
    let helper_pos = items.iter().position(|i| i.label == "StringHelper");

    assert!(string_pos.is_some(), "expected 'string' in results");
    assert!(helper_pos.is_some(), "expected 'StringHelper' in results");

    // `string` must appear before `StringHelper` in the sorted list.
    // LSP clients sort by sort_text, so compare those directly.
    let string_sort = items[string_pos.unwrap()].sort_text.as_deref().unwrap();
    let helper_sort = items[helper_pos.unwrap()].sort_text.as_deref().unwrap();
    assert!(
        string_sort < helper_sort,
        "scalar 'string' (sort_text={:?}) should sort before class 'StringHelper' (sort_text={:?})",
        string_sort,
        helper_sort
    );
}

// ─── Type hint excludes traits ──────────────────────────────────────────────

/// Traits should not appear in parameter type-hint completions.
/// PHP accepts the syntax but the type check always fails at runtime.
#[tokio::test]
async fn param_type_excludes_traits() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/type_hint_trait.php").unwrap();

    let src = concat!(
        "<?php\n",
        "namespace TypeHintTest;\n",
        "class SomeClass {}\n",
        "interface SomeInterface {}\n",
        "trait SomeTrait {}\n",
        "enum SomeEnum {}\n",
        "function demo(Some) {}\n",
    );
    // Line 6: `function demo(Some) {}`
    // cursor after "Some" = col 18
    let items = complete_at(&backend, &uri, src, 6, 18).await;
    let cls = class_items(&items);
    let lbls: Vec<&str> = cls.iter().map(|i| i.label.as_str()).collect();

    assert!(
        lbls.contains(&"SomeClass"),
        "type hint should include classes, got: {:?}",
        lbls
    );
    assert!(
        lbls.contains(&"SomeInterface"),
        "type hint should include interfaces, got: {:?}",
        lbls
    );
    assert!(
        lbls.contains(&"SomeEnum"),
        "type hint should include enums, got: {:?}",
        lbls
    );
    assert!(
        !lbls.contains(&"SomeTrait"),
        "type hint should NOT include traits, got: {:?}",
        lbls
    );
}

/// Traits should not appear in return type-hint completions.
#[tokio::test]
async fn return_type_excludes_traits() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/type_hint_return_trait.php").unwrap();

    let src = concat!(
        "<?php\n",
        "namespace ReturnHintTest;\n",
        "class SomeClass {}\n",
        "trait SomeTrait {}\n",
        "function demo(): Some {}\n",
    );
    // Line 4: `function demo(): Some {}`
    // cursor after "Some" = col 21
    let items = complete_at(&backend, &uri, src, 4, 21).await;
    let cls = class_items(&items);
    let lbls: Vec<&str> = cls.iter().map(|i| i.label.as_str()).collect();

    assert!(
        lbls.contains(&"SomeClass"),
        "return type should include classes, got: {:?}",
        lbls
    );
    assert!(
        !lbls.contains(&"SomeTrait"),
        "return type should NOT include traits, got: {:?}",
        lbls
    );
}

/// Traits should not appear in property type-hint completions.
#[tokio::test]
async fn property_type_excludes_traits() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/type_hint_prop_trait.php").unwrap();

    let src = concat!(
        "<?php\n",
        "namespace PropHintTest;\n",
        "class SomeClass {}\n",
        "trait SomeTrait {}\n",
        "class Demo {\n",
        "    public Some\n",
        "}\n",
    );
    // Line 5: `    public Some`
    // cursor after "Some" = col 15
    let items = complete_at(&backend, &uri, src, 5, 15).await;
    let cls = class_items(&items);
    let lbls: Vec<&str> = cls.iter().map(|i| i.label.as_str()).collect();

    assert!(
        lbls.contains(&"SomeClass"),
        "property type should include classes, got: {:?}",
        lbls
    );
    assert!(
        !lbls.contains(&"SomeTrait"),
        "property type should NOT include traits, got: {:?}",
        lbls
    );
}

/// Union type hints should also exclude traits.
#[tokio::test]
async fn union_type_excludes_traits() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/type_hint_union_trait.php").unwrap();

    let src = concat!(
        "<?php\n",
        "namespace UnionHintTest;\n",
        "class SomeClass {}\n",
        "trait SomeTrait {}\n",
        "function demo(string|Some) {}\n",
    );
    // Line 4: `function demo(string|Some) {}`
    // cursor after "Some" = col 25
    let items = complete_at(&backend, &uri, src, 4, 25).await;
    let cls = class_items(&items);
    let lbls: Vec<&str> = cls.iter().map(|i| i.label.as_str()).collect();

    assert!(
        lbls.contains(&"SomeClass"),
        "union type should include classes, got: {:?}",
        lbls
    );
    assert!(
        !lbls.contains(&"SomeTrait"),
        "union type should NOT include traits, got: {:?}",
        lbls
    );
}
