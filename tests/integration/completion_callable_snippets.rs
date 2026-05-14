use crate::common::{create_test_backend, create_test_backend_with_function_stubs};
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

// ─── Helper ─────────────────────────────────────────────────────────────────

async fn complete_at(
    backend: &phpantom_lsp::Backend,
    uri: &Url,
    text: &str,
    line: u32,
    character: u32,
) -> Vec<CompletionItem> {
    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    backend.did_open(open_params).await;

    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position { line, character },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    match backend.completion(completion_params).await.unwrap() {
        Some(CompletionResponse::Array(items)) => items,
        Some(CompletionResponse::List(list)) => list.items,
        None => Vec::new(),
    }
}

fn find_method<'a>(items: &'a [CompletionItem], name: &str) -> Option<&'a CompletionItem> {
    items.iter().find(|i| {
        i.kind == Some(CompletionItemKind::METHOD) && i.filter_text.as_deref() == Some(name)
    })
}

fn find_function<'a>(items: &'a [CompletionItem], name: &str) -> Option<&'a CompletionItem> {
    items.iter().find(|i| {
        i.kind == Some(CompletionItemKind::FUNCTION) && i.filter_text.as_deref() == Some(name)
    })
}

fn find_class<'a>(items: &'a [CompletionItem], name: &str) -> Option<&'a CompletionItem> {
    items.iter().find(|i| {
        i.kind == Some(CompletionItemKind::CLASS)
            && (i.label == name || i.detail.as_deref() == Some(name))
    })
}

// ─── Method Snippet Tests ───────────────────────────────────────────────────

/// Methods with no parameters should get `()$0` as snippet.
#[tokio::test]
async fn test_snippet_method_no_params() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///snip_no_params.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Foo {\n",
        "    public function doStuff(): void {}\n",
        "    public function test() {\n",
        "        $this->\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 4, 15).await;
    let item = find_method(&items, "doStuff").expect("Should find doStuff");

    assert_eq!(item.insert_text.as_deref(), Some("doStuff()$0"));
    assert_eq!(item.insert_text_format, Some(InsertTextFormat::SNIPPET));
}

/// A method with one required param should get `(${1:\$name})$0`.
#[tokio::test]
async fn test_snippet_method_one_required_param() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///snip_one_req.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Greeter {\n",
        "    public function greet(string $name): string {}\n",
        "    public function test() {\n",
        "        $this->\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 4, 15).await;
    let item = find_method(&items, "greet").expect("Should find greet");

    assert_eq!(item.insert_text.as_deref(), Some("greet(${1:\\$name})$0"));
    assert_eq!(item.insert_text_format, Some(InsertTextFormat::SNIPPET));
}

/// A method with two required params should get `(${1:\$a}, ${2:\$b})$0`.
#[tokio::test]
async fn test_snippet_method_two_required_params() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///snip_two_req.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Math {\n",
        "    public function add(int $a, int $b): int {}\n",
        "    public function test() {\n",
        "        $this->\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 4, 15).await;
    let item = find_method(&items, "add").expect("Should find add");

    assert_eq!(
        item.insert_text.as_deref(),
        Some("add(${1:\\$a}, ${2:\\$b})$0")
    );
    assert_eq!(item.insert_text_format, Some(InsertTextFormat::SNIPPET));
}

/// Optional params are excluded — only required params appear in the snippet.
#[tokio::test]
async fn test_snippet_method_mixed_required_and_optional() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///snip_mixed.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Editor {\n",
        "    public function replace(string $search, string $replace, bool $caseSensitive = true): string {}\n",
        "    public function test() {\n",
        "        $this->\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 4, 15).await;
    let item = find_method(&items, "replace").expect("Should find replace");

    assert_eq!(
        item.insert_text.as_deref(),
        Some("replace(${1:\\$search}, ${2:\\$replace})$0"),
        "Optional $caseSensitive should be excluded from snippet"
    );
    assert_eq!(item.insert_text_format, Some(InsertTextFormat::SNIPPET));
}

/// All-optional params should produce empty parens `()$0`.
#[tokio::test]
async fn test_snippet_method_all_optional() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///snip_all_opt.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Config {\n",
        "    public function setup($debug = false, $verbose = false): void {}\n",
        "    public function test() {\n",
        "        $this->\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 4, 15).await;
    let item = find_method(&items, "setup").expect("Should find setup");

    assert_eq!(
        item.insert_text.as_deref(),
        Some("setup()$0"),
        "All-optional params should produce empty parens"
    );
    assert_eq!(item.insert_text_format, Some(InsertTextFormat::SNIPPET));
}

/// Variadic params are not required — they should be excluded from the snippet.
#[tokio::test]
async fn test_snippet_method_variadic_excluded() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///snip_variadic.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Logger {\n",
        "    public function log(string $message, ...$context): void {}\n",
        "    public function logAll(...$messages): void {}\n",
        "    public function test() {\n",
        "        $this->\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 5, 15).await;

    let log = find_method(&items, "log").expect("Should find log");
    assert_eq!(
        log.insert_text.as_deref(),
        Some("log(${1:\\$message})$0"),
        "Variadic ...$context should be excluded"
    );

    let log_all = find_method(&items, "logAll").expect("Should find logAll");
    assert_eq!(
        log_all.insert_text.as_deref(),
        Some("logAll()$0"),
        "Only-variadic should produce empty parens"
    );
}

/// filter_text should still be just the method name (for fuzzy matching).
#[tokio::test]
async fn test_snippet_filter_text_is_plain_name() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///snip_filter.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Svc {\n",
        "    public function process(int $id, string $data): bool {}\n",
        "    public function test() {\n",
        "        $this->\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 4, 15).await;
    let item = find_method(&items, "process").expect("Should find process");

    assert_eq!(
        item.filter_text.as_deref(),
        Some("process"),
        "filter_text should be just the method name, not a snippet"
    );
}

/// Static methods via `::` should also get snippets.
#[tokio::test]
async fn test_snippet_static_method() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///snip_static.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Factory {\n",
        "    public static function create(string $type): self {}\n",
        "}\n",
        "class Client {\n",
        "    public function run() {\n",
        "        Factory::\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 6, 17).await;
    let item = find_method(&items, "create").expect("Should find create");

    assert_eq!(item.insert_text.as_deref(), Some("create(${1:\\$type})$0"));
    assert_eq!(item.insert_text_format, Some(InsertTextFormat::SNIPPET));
}

/// Methods accessed via a variable type-hinted by a class should get snippets.
#[tokio::test]
async fn test_snippet_method_via_variable() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///snip_var.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Mailer {\n",
        "    public function send(string $to, string $subject): bool {}\n",
        "}\n",
        "class App {\n",
        "    public function run(Mailer $mailer) {\n",
        "        $mailer->\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 6, 17).await;
    let item = find_method(&items, "send").expect("Should find send");

    assert_eq!(
        item.insert_text.as_deref(),
        Some("send(${1:\\$to}, ${2:\\$subject})$0")
    );
    assert_eq!(item.insert_text_format, Some(InsertTextFormat::SNIPPET));
}

/// Reference parameters should still appear in the snippet.
#[tokio::test]
async fn test_snippet_method_reference_param() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///snip_ref.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Parser {\n",
        "    public function parse(string $input, array &$errors): bool {}\n",
        "    public function test() {\n",
        "        $this->\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 4, 15).await;
    let item = find_method(&items, "parse").expect("Should find parse");

    assert_eq!(
        item.insert_text.as_deref(),
        Some("parse(${1:\\$input}, ${2:\\$errors})$0"),
        "Reference params are still required and should appear in snippet"
    );
}

// ─── Function Snippet Tests ─────────────────────────────────────────────────

/// User-defined functions with required params should get snippets.
#[tokio::test]
async fn test_snippet_user_function_with_params() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///snip_func.php").unwrap();
    let text = concat!(
        "<?php\n",
        "function makeText(string $text, $long = false): string {}\n",
        "makeTe\n",
    );

    let items = complete_at(&backend, &uri, text, 2, 6).await;
    let item = find_function(&items, "makeText").expect("Should find makeText");

    assert_eq!(
        item.insert_text.as_deref(),
        Some("makeText(${1:\\$text})$0"),
        "Only the required $text param should be in the snippet"
    );
    assert_eq!(item.insert_text_format, Some(InsertTextFormat::SNIPPET));
}

/// User-defined functions with no params should get `()$0`.
#[tokio::test]
async fn test_snippet_user_function_no_params() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///snip_func_no.php").unwrap();
    let text = concat!("<?php\n", "function getVersion(): string {}\n", "getVe\n",);

    let items = complete_at(&backend, &uri, text, 2, 5).await;
    let item = find_function(&items, "getVersion").expect("Should find getVersion");

    assert_eq!(item.insert_text.as_deref(), Some("getVersion()$0"));
    assert_eq!(item.insert_text_format, Some(InsertTextFormat::SNIPPET));
}

/// Stub functions (built-in PHP functions) get `name()$0` — we know they're
/// callable but don't have parameter info loaded, so just insert empty parens.
#[tokio::test]
async fn test_snippet_stub_function_empty_parens() {
    let backend = create_test_backend_with_function_stubs();
    let uri = Url::parse("file:///snip_stub.php").unwrap();
    let text = concat!("<?php\n", "json_d\n",);

    let items = complete_at(&backend, &uri, text, 1, 6).await;
    let item = items.iter().find(|i| {
        i.kind == Some(CompletionItemKind::FUNCTION)
            && i.filter_text.as_deref() == Some("json_decode")
    });
    let item = item.expect("Should find json_decode");

    assert_eq!(
        item.insert_text.as_deref(),
        Some("json_decode()$0"),
        "Stub functions should insert name with empty parens snippet"
    );
    assert_eq!(
        item.insert_text_format,
        Some(InsertTextFormat::SNIPPET),
        "Stub functions should use snippet format"
    );
}

/// User-defined function with multiple required params.
#[tokio::test]
async fn test_snippet_user_function_multiple_required() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///snip_func_multi.php").unwrap();
    let text = concat!(
        "<?php\n",
        "function clamp(int $value, int $min, int $max): int {}\n",
        "clam\n",
    );

    let items = complete_at(&backend, &uri, text, 2, 4).await;
    let item = find_function(&items, "clamp").expect("Should find clamp");

    assert_eq!(
        item.insert_text.as_deref(),
        Some("clamp(${1:\\$value}, ${2:\\$min}, ${3:\\$max})$0")
    );
    assert_eq!(item.insert_text_format, Some(InsertTextFormat::SNIPPET));
}

// ─── `new ClassName` Snippet Tests ──────────────────────────────────────────

/// Non-namespaced classes in the same file are available via ast_map (source 2),
/// so they get constructor params included in the snippet.
#[tokio::test]
async fn test_snippet_new_class_non_namespaced_with_constructor_params() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///snip_new.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class MoneyFactory {\n",
        "    public function __construct(int $amount) {}\n",
        "}\n",
        "new Mon\n",
    );

    let items = complete_at(&backend, &uri, text, 4, 7).await;
    let item = find_class(&items, "MoneyFactory").expect("Should find MoneyFactory");

    assert_eq!(
        item.insert_text.as_deref(),
        Some("MoneyFactory(${1:\\$amount})$0"),
        "Non-namespaced class in same file has constructor info available"
    );
    assert_eq!(item.insert_text_format, Some(InsertTextFormat::SNIPPET));
}

/// Same-namespace classes are found via ast_map (source 2), so the
/// constructor parameters ARE available and included in the snippet.
#[tokio::test]
async fn test_snippet_new_class_namespaced_gets_constructor_params() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///snip_new_ns.php").unwrap();
    let text = concat!(
        "<?php\n",
        "namespace App;\n",
        "class MoneyFactory {\n",
        "    public function __construct(int $amount) {}\n",
        "}\n",
        "new Mon\n",
    );

    let items = complete_at(&backend, &uri, text, 5, 7).await;
    let item = find_class(&items, "App\\MoneyFactory").expect("Should find MoneyFactory");

    assert_eq!(
        item.insert_text.as_deref(),
        Some("MoneyFactory(${1:\\$amount})$0"),
        "Same-namespace class should include constructor required params"
    );
    assert_eq!(item.insert_text_format, Some(InsertTextFormat::SNIPPET));
}

/// `new` context with a class that has no constructor.
#[tokio::test]
async fn test_snippet_new_class_no_constructor() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///snip_new_noctor.php").unwrap();
    let text = concat!("<?php\n", "class SimpleObj {}\n", "new Sim\n",);

    let items = complete_at(&backend, &uri, text, 2, 7).await;
    let item = find_class(&items, "SimpleObj").expect("Should find SimpleObj");

    assert_eq!(
        item.insert_text.as_deref(),
        Some("SimpleObj()$0"),
        "Class with no constructor should still get empty parens"
    );
    assert_eq!(item.insert_text_format, Some(InsertTextFormat::SNIPPET));
}

/// Same-namespace class with mixed required and optional constructor params.
#[tokio::test]
async fn test_snippet_new_class_mixed_params() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///snip_new_mixed.php").unwrap();
    let text = concat!(
        "<?php\n",
        "namespace App;\n",
        "class Connection {\n",
        "    public function __construct(string $host, int $port = 3306, bool $ssl = false) {}\n",
        "}\n",
        "new Conn\n",
    );

    let items = complete_at(&backend, &uri, text, 5, 8).await;
    let item = find_class(&items, "App\\Connection").expect("Should find Connection");

    assert_eq!(
        item.insert_text.as_deref(),
        Some("Connection(${1:\\$host})$0"),
        "Only required $host should appear, optional $port and $ssl excluded"
    );
    assert_eq!(item.insert_text_format, Some(InsertTextFormat::SNIPPET));
}

/// Same-namespace class with all optional constructor params.
#[tokio::test]
async fn test_snippet_new_class_all_optional_constructor() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///snip_new_allopt.php").unwrap();
    let text = concat!(
        "<?php\n",
        "namespace App;\n",
        "class Options {\n",
        "    public function __construct($debug = false, $verbose = false) {}\n",
        "}\n",
        "new Opt\n",
    );

    let items = complete_at(&backend, &uri, text, 5, 7).await;
    let item = find_class(&items, "App\\Options").expect("Should find Options");

    assert_eq!(
        item.insert_text.as_deref(),
        Some("Options()$0"),
        "All-optional constructor should produce empty parens"
    );
    assert_eq!(item.insert_text_format, Some(InsertTextFormat::SNIPPET));
}

/// Same-namespace class with multiple required constructor params.
#[tokio::test]
async fn test_snippet_new_class_multiple_required() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///snip_new_multi.php").unwrap();
    let text = concat!(
        "<?php\n",
        "namespace Geo;\n",
        "class Point {\n",
        "    public function __construct(float $x, float $y, float $z) {}\n",
        "}\n",
        "new Poi\n",
    );

    let items = complete_at(&backend, &uri, text, 5, 7).await;
    let item = find_class(&items, "Geo\\Point").expect("Should find Point");

    assert_eq!(
        item.insert_text.as_deref(),
        Some("Point(${1:\\$x}, ${2:\\$y}, ${3:\\$z})$0")
    );
    assert_eq!(item.insert_text_format, Some(InsertTextFormat::SNIPPET));
}

/// Outside `new` context, class name completion should NOT add parens.
#[tokio::test]
async fn test_snippet_class_name_no_new_context() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///snip_no_new.php").unwrap();
    let text = concat!(
        "<?php\n",
        "namespace App;\n",
        "class Widget {\n",
        "    public function __construct(string $name) {}\n",
        "}\n",
        "class Service {\n",
        "    public function run(Wid) {}\n",
        "}\n",
    );

    // Cursor at the type-hint position in the method signature
    let items = complete_at(&backend, &uri, text, 6, 27).await;
    let widget = items
        .iter()
        .find(|i| i.kind == Some(CompletionItemKind::CLASS) && i.label == "App\\Widget");

    if let Some(item) = widget {
        // When not in `new` context, insert_text should be the plain class name
        assert_eq!(
            item.insert_text.as_deref(),
            Some("Widget"),
            "Outside `new` context, class name should not have parens"
        );
        assert_ne!(
            item.insert_text_format,
            Some(InsertTextFormat::SNIPPET),
            "Outside `new` context, should not use snippet format"
        );
    }
}

// ─── Inherited Method Snippet Tests ─────────────────────────────────────────

/// Methods inherited from a parent class should also get snippets.
#[tokio::test]
async fn test_snippet_inherited_method() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///snip_inherit.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Base {\n",
        "    public function save(string $path): bool {}\n",
        "}\n",
        "class Child extends Base {\n",
        "    public function test() {\n",
        "        $this->\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 6, 15).await;
    let item = find_method(&items, "save").expect("Should find inherited save");

    assert_eq!(
        item.insert_text.as_deref(),
        Some("save(${1:\\$path})$0"),
        "Inherited methods should also get parameter snippets"
    );
    assert_eq!(item.insert_text_format, Some(InsertTextFormat::SNIPPET));
}

/// parent:: constructor calls should also get parameter snippets.
#[tokio::test]
async fn test_snippet_parent_constructor() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///snip_parent.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Animal {\n",
        "    public function __construct(string $name) {}\n",
        "}\n",
        "class Dog extends Animal {\n",
        "    public function __construct(string $name, string $breed) {\n",
        "        parent::\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 6, 16).await;
    let item = find_method(&items, "__construct").expect("Should find parent __construct");

    assert_eq!(
        item.insert_text.as_deref(),
        Some("__construct(${1:\\$name})$0"),
        "parent::__construct should include required params"
    );
    assert_eq!(item.insert_text_format, Some(InsertTextFormat::SNIPPET));
}

// ─── `new` Context Inside Class Method ──────────────────────────────────────

/// `new ClassName` inside a class method should get constructor snippets
/// when the target class is in the same namespace (ast_map lookup).
#[tokio::test]
async fn test_snippet_new_inside_method_same_namespace() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///snip_new_method.php").unwrap();
    let text = concat!(
        "<?php\n",
        "namespace App;\n",
        "class Logger {\n",
        "    public function __construct(string $channel) {}\n",
        "}\n",
        "class App {\n",
        "    public function boot() {\n",
        "        $log = new Log\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 7, 22).await;
    let item = find_class(&items, "App\\Logger").expect("Should find Logger");

    assert_eq!(
        item.insert_text.as_deref(),
        Some("Logger(${1:\\$channel})$0"),
    );
    assert_eq!(item.insert_text_format, Some(InsertTextFormat::SNIPPET));
}

/// `new ClassName` for a non-namespaced class in the same file has
/// constructor info available via ast_map (source 2).
#[tokio::test]
async fn test_snippet_new_inside_method_non_namespaced() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///snip_new_noname.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Logger {\n",
        "    public function __construct(string $channel) {}\n",
        "}\n",
        "class App {\n",
        "    public function boot() {\n",
        "        $log = new Log\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 6, 22).await;
    let item = find_class(&items, "Logger").expect("Should find Logger");

    assert_eq!(
        item.insert_text.as_deref(),
        Some("Logger(${1:\\$channel})$0"),
        "Non-namespaced class in same file has constructor info available"
    );
    assert_eq!(item.insert_text_format, Some(InsertTextFormat::SNIPPET));
}

// ─── Properties and Constants (no snippets) ─────────────────────────────────

/// Properties should NOT get snippet format — they're not callable.
#[tokio::test]
async fn test_no_snippet_for_properties() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///snip_prop.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Box {\n",
        "    public int $width = 0;\n",
        "    public function test() {\n",
        "        $this->\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 4, 15).await;
    let prop = items
        .iter()
        .find(|i| i.kind == Some(CompletionItemKind::PROPERTY) && i.label == "width")
        .expect("Should find width property");

    assert_eq!(
        prop.insert_text.as_deref(),
        Some("width"),
        "Properties should not have snippet parens"
    );
    assert_ne!(
        prop.insert_text_format,
        Some(InsertTextFormat::SNIPPET),
        "Properties should not use snippet format"
    );
}

/// Constants should NOT get snippet format — they're not callable.
#[tokio::test]
async fn test_no_snippet_for_constants() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///snip_const.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Status {\n",
        "    const ACTIVE = 1;\n",
        "}\n",
        "class Client {\n",
        "    public function run() {\n",
        "        Status::\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 6, 16).await;
    let c = items
        .iter()
        .find(|i| i.kind == Some(CompletionItemKind::CONSTANT) && i.label == "ACTIVE")
        .expect("Should find ACTIVE constant");

    assert_eq!(c.insert_text.as_deref(), Some("ACTIVE"));
    assert_ne!(
        c.insert_text_format,
        Some(InsertTextFormat::SNIPPET),
        "Constants should not use snippet format"
    );
}

// ─── `throw new` Context ────────────────────────────────────────────────────

/// `throw new ExceptionClass` should get parens in the snippet.
/// The throw-new path goes through `build_class_name_completions`
/// with `ClassNameContext::ThrowNew`, so at minimum we get `Name()$0`.
#[tokio::test]
async fn test_snippet_throw_new_context() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///snip_throw.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class AppException extends \\Exception {\n",
        "    public function __construct(string $msg, int $code) {\n",
        "        parent::__construct($msg, $code);\n",
        "    }\n",
        "}\n",
        "throw new AppE\n",
    );

    let items = complete_at(&backend, &uri, text, 6, 14).await;
    let item = find_class(&items, "AppException");

    if let Some(item) = item {
        let insert = item.insert_text.as_deref().unwrap_or("");
        assert!(
            insert.contains("AppException("),
            "throw new context should include parens: got '{}'",
            insert,
        );
        assert_eq!(
            item.insert_text_format,
            Some(InsertTextFormat::SNIPPET),
            "throw new context should use snippet format"
        );
    }
}

// ─── Suppress parentheses when `(` already follows cursor ───────────────────

/// When completing `$obj->|()`, the parentheses already exist after the
/// cursor.  The completion item should insert just the method name (plain
/// text) instead of a snippet with `()`, which would produce `method()()`.
#[tokio::test]
async fn test_snippet_suppressed_when_parens_follow_cursor() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///snip_paren_follows.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Gadget {\n",
        "    public function doStuff(): void {}\n",
        "    public function run(): void {}\n",
        "}\n",
        "$g = new Gadget();\n",
        "$g->()\n",
    );

    // Cursor is right after `->`, before `()`.
    let items = complete_at(&backend, &uri, text, 6, 4).await;
    let item = find_method(&items, "doStuff").expect("Should find doStuff");

    // Insert text should be the plain method name without `()`.
    assert_eq!(
        item.insert_text.as_deref(),
        Some("doStuff"),
        "should insert plain name when parens already follow"
    );
    // Format should NOT be snippet (or should be absent).
    assert!(
        item.insert_text_format != Some(InsertTextFormat::SNIPPET),
        "should not use snippet format when parens already follow"
    );
}

/// Same suppression should work when the user has partially typed the
/// method name: `$obj->doSt|()`.
#[tokio::test]
async fn test_snippet_suppressed_when_parens_follow_partial_identifier() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///snip_paren_partial.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Widget {\n",
        "    public function calculate(int $x): int { return $x; }\n",
        "}\n",
        "$w = new Widget();\n",
        "$w->calc()\n",
    );

    // Cursor after `calc`, before `()`.
    let items = complete_at(&backend, &uri, text, 5, 8).await;
    let item = find_method(&items, "calculate").expect("Should find calculate");

    assert_eq!(
        item.insert_text.as_deref(),
        Some("calculate"),
        "should insert plain name when parens follow partial identifier"
    );
    assert!(
        item.insert_text_format != Some(InsertTextFormat::SNIPPET),
        "should not use snippet format when parens follow partial identifier"
    );
}

/// When there are no parentheses after the cursor, snippets should still
/// include `()` as usual.
#[tokio::test]
async fn test_snippet_preserved_when_no_parens_follow() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///snip_no_paren.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Gizmo {\n",
        "    public function doStuff(): void {}\n",
        "}\n",
        "$g = new Gizmo();\n",
        "$g->\n",
    );

    let items = complete_at(&backend, &uri, text, 5, 4).await;
    let item = find_method(&items, "doStuff").expect("Should find doStuff");

    assert_eq!(
        item.insert_text.as_deref(),
        Some("doStuff()$0"),
        "should include parens when none follow the cursor"
    );
    assert_eq!(
        item.insert_text_format,
        Some(InsertTextFormat::SNIPPET),
        "should use snippet format when no parens follow"
    );
}

// ─── Suppress parentheses for standalone function calls ─────────────────────

/// When completing `array_m|()`, the parentheses already exist.
/// The function completion should insert just the name, not a snippet.
#[tokio::test]
async fn test_snippet_suppressed_for_function_when_parens_follow() {
    let backend = create_test_backend_with_function_stubs();
    let uri = Url::parse("file:///func_paren_follows.php").unwrap();
    let text = concat!("<?php\n", "array_m()\n",);

    // Cursor after `array_m`, before `()`.
    let items = complete_at(&backend, &uri, text, 1, 7).await;
    let item = find_function(&items, "array_map").expect("Should find array_map");

    assert_eq!(
        item.insert_text.as_deref(),
        Some("array_map"),
        "should insert plain function name when parens already follow"
    );
    assert!(
        item.insert_text_format != Some(InsertTextFormat::SNIPPET),
        "should not use snippet format for function when parens already follow"
    );
}

/// When completing a function call without existing parens, the snippet
/// should still include `()`.
#[tokio::test]
async fn test_snippet_preserved_for_function_when_no_parens_follow() {
    let backend = create_test_backend_with_function_stubs();
    let uri = Url::parse("file:///func_no_paren.php").unwrap();
    let text = concat!("<?php\n", "array_m\n",);

    let items = complete_at(&backend, &uri, text, 1, 7).await;
    let item = find_function(&items, "array_map").expect("Should find array_map");

    assert_eq!(
        item.insert_text_format,
        Some(InsertTextFormat::SNIPPET),
        "should use snippet format for function when no parens follow"
    );
    assert!(
        item.insert_text.as_deref().unwrap_or("").contains('('),
        "should include parens in snippet when none follow"
    );
}

// ─── Suppress parentheses for `new ClassName()` ─────────────────────────────

/// When completing `new Gadge|()`, the parentheses already exist.
/// The class completion should insert just the name, not a snippet.
#[tokio::test]
async fn test_snippet_suppressed_for_new_when_parens_follow() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///new_paren_follows.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Gadget {\n",
        "    public function __construct(int $x) {}\n",
        "}\n",
        "$g = new Gadge()\n",
    );

    // Cursor after `Gadge`, before `()`.  Line 4, col 15 = right after `Gadge`.
    let items = complete_at(&backend, &uri, text, 4, 14).await;
    let item = find_class(&items, "Gadget").expect("Should find Gadget");

    assert_eq!(
        item.insert_text.as_deref(),
        Some("Gadget"),
        "should insert plain class name when parens already follow in new expression"
    );
    assert!(
        item.insert_text_format != Some(InsertTextFormat::SNIPPET),
        "should not use snippet format for new when parens already follow"
    );
}

/// When completing `new Gadge` without trailing parens, the snippet
/// should still include `()`.
#[tokio::test]
async fn test_snippet_preserved_for_new_when_no_parens_follow() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///new_no_paren.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Gadget {\n",
        "    public function __construct(int $x) {}\n",
        "}\n",
        "$g = new Gadge\n",
    );

    let items = complete_at(&backend, &uri, text, 4, 14).await;
    let item = find_class(&items, "Gadget").expect("Should find Gadget");

    assert_eq!(
        item.insert_text_format,
        Some(InsertTextFormat::SNIPPET),
        "should use snippet format for new when no parens follow"
    );
    assert!(
        item.insert_text.as_deref().unwrap_or("").contains('('),
        "should include parens in snippet for new when none follow"
    );
}

// ─── Suppress parentheses for `throw new Exception()` ───────────────────────

/// When completing `throw new Excepti|()`, the parens already exist.
#[tokio::test]
async fn test_snippet_suppressed_for_throw_new_when_parens_follow() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///throw_paren_follows.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class MyException extends \\Exception {}\n",
        "function boom() {\n",
        "    throw new MyExcepti();\n",
        "}\n",
    );

    // Cursor after `MyExcepti`, before `()`.
    let items = complete_at(&backend, &uri, text, 3, 23).await;
    let item = find_class(&items, "MyException").expect("Should find MyException");

    assert_eq!(
        item.insert_text.as_deref(),
        Some("MyException"),
        "should insert plain class name for throw new when parens already follow"
    );
    assert!(
        item.insert_text_format != Some(InsertTextFormat::SNIPPET),
        "should not use snippet format for throw new when parens already follow"
    );
}

// ─── Suppress parentheses for static method calls `Class::method()` ────────

/// When completing `Gadget::doSt|()`, the parens already exist.
#[tokio::test]
async fn test_snippet_suppressed_for_static_call_when_parens_follow() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///static_paren_follows.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Gadget {\n",
        "    public static function doStuff(int $x): void {}\n",
        "}\n",
        "Gadget::doSt()\n",
    );

    // Cursor after `doSt`, before `()`.
    let items = complete_at(&backend, &uri, text, 4, 12).await;
    let item = find_method(&items, "doStuff").expect("Should find doStuff");

    assert_eq!(
        item.insert_text.as_deref(),
        Some("doStuff"),
        "should insert plain name for static call when parens already follow"
    );
    assert!(
        item.insert_text_format != Some(InsertTextFormat::SNIPPET),
        "should not use snippet format for static call when parens already follow"
    );
}

// ─── Class Keywords (self, static, parent) in new expressions ───────────────

/// `new self` should be offered with constructor snippet when inside a class.
#[tokio::test]
async fn test_snippet_new_self_with_constructor() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///new_self.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Config {\n",
        "    public function __construct(string $host, int $port) {}\n",
        "    public static function create(): self {\n",
        "        return new sel\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 4, 22).await;
    let item = items
        .iter()
        .find(|i| i.kind == Some(CompletionItemKind::KEYWORD) && i.label == "self")
        .expect("Should find 'self' keyword");

    assert_eq!(
        item.insert_text.as_deref(),
        Some("self(${1:\\$host}, ${2:\\$port})$0"),
        "self should include constructor parameters"
    );
    assert_eq!(item.insert_text_format, Some(InsertTextFormat::SNIPPET));
    assert!(
        item.detail
            .as_ref()
            .unwrap()
            .contains("Instantiate current class")
    );
}

/// `new static` should be offered with constructor snippet when inside a class.
#[tokio::test]
async fn test_snippet_new_static_with_constructor() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///new_static.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Factory {\n",
        "    public function __construct(array $options, bool $debug = false) {}\n",
        "    public static function make(): static {\n",
        "        return new sta\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 4, 22).await;
    let item = items
        .iter()
        .find(|i| i.kind == Some(CompletionItemKind::KEYWORD) && i.label == "static")
        .expect("Should find 'static' keyword");

    assert_eq!(
        item.insert_text.as_deref(),
        Some("static(${1:\\$options})$0"),
        "static should include required constructor parameters only"
    );
    assert_eq!(item.insert_text_format, Some(InsertTextFormat::SNIPPET));
}

/// `new parent` should be offered with parent constructor snippet when inside a child class.
#[tokio::test]
async fn test_snippet_new_parent_with_constructor() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///new_parent.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Animal {\n",
        "    public function __construct(string $name, int $age) {}\n",
        "}\n",
        "class Dog extends Animal {\n",
        "    public function test(): void {\n",
        "        $x = new par\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 6, 19).await;
    let item = items
        .iter()
        .find(|i| i.kind == Some(CompletionItemKind::KEYWORD) && i.label == "parent")
        .expect("Should find 'parent' keyword");

    assert_eq!(
        item.insert_text.as_deref(),
        Some("parent(${1:\\$name}, ${2:\\$age})$0"),
        "parent should include parent class constructor parameters"
    );
    assert_eq!(item.insert_text_format, Some(InsertTextFormat::SNIPPET));
    assert!(item.detail.as_ref().unwrap().contains("Animal"));
}

/// `new self` without constructor should offer empty parens snippet.
#[tokio::test]
async fn test_snippet_new_self_no_constructor() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///new_self_noctor.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Simple {\n",
        "    public static function create(): self {\n",
        "        return new sel\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 3, 22).await;
    let item = items
        .iter()
        .find(|i| i.kind == Some(CompletionItemKind::KEYWORD) && i.label == "self")
        .expect("Should find 'self' keyword");

    assert_eq!(
        item.insert_text.as_deref(),
        Some("self()$0"),
        "self without constructor should have empty parens"
    );
    assert_eq!(item.insert_text_format, Some(InsertTextFormat::SNIPPET));
}

/// `parent` should not be offered when class has no parent.
#[tokio::test]
async fn test_snippet_new_parent_not_offered_without_parent() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///new_no_parent.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Standalone {\n",
        "    public function test(): void {\n",
        "        $x = new par\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 3, 20).await;
    let parent_item = items
        .iter()
        .find(|i| i.kind == Some(CompletionItemKind::KEYWORD) && i.label == "parent");

    assert!(
        parent_item.is_none(),
        "parent should not be offered when class has no parent"
    );
}

/// `self`/`static`/`parent` should not be offered outside a class context.
#[tokio::test]
async fn test_snippet_new_keywords_not_offered_outside_class() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///new_outside_class.php").unwrap();
    let text = concat!(
        "<?php\n",
        "function test(): void {\n",
        "    $x = new sel\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 2, 16).await;
    let self_item = items
        .iter()
        .find(|i| i.kind == Some(CompletionItemKind::KEYWORD) && i.label == "self");
    let static_item = items
        .iter()
        .find(|i| i.kind == Some(CompletionItemKind::KEYWORD) && i.label == "static");

    assert!(
        self_item.is_none(),
        "self should not be offered outside a class"
    );
    assert!(
        static_item.is_none(),
        "static should not be offered outside a class"
    );
}
