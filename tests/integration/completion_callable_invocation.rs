use crate::common::{complete_at, create_psr4_workspace, create_test_backend};
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

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

// ─── Closure literal with native return type hint ───────────────────────────

#[tokio::test]
async fn test_closure_literal_return_type() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/closure_invoke.php").unwrap();

    let src = concat!(
        "<?php\n",
        "class User {\n",
        "    public function getName(): string { return ''; }\n",
        "    public function getEmail(): string { return ''; }\n",
        "}\n",
        "class Service {\n",
        "    public function run(): void {\n",
        "        $fn = function(): User { return new User(); };\n",
        "        $fn()->\n",
        "    }\n",
        "}\n",
    );

    // Line 8: `        $fn()->`  cursor after `->`
    let items = complete_at(&backend, &uri, src, 8, 15).await;
    let names = method_names(&items);
    assert!(
        names.contains(&"getName"),
        "Expected getName in {:?}",
        names,
    );
    assert!(
        names.contains(&"getEmail"),
        "Expected getEmail in {:?}",
        names,
    );
}

#[tokio::test]
async fn test_arrow_function_literal_return_type() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/arrow_invoke.php").unwrap();

    let src = concat!(
        "<?php\n",
        "class Product {\n",
        "    public function getPrice(): float { return 0.0; }\n",
        "    public function getTitle(): string { return ''; }\n",
        "}\n",
        "class Service {\n",
        "    public function run(): void {\n",
        "        $factory = fn(): Product => new Product();\n",
        "        $factory()->\n",
        "    }\n",
        "}\n",
    );

    // Line 8: `        $factory()->` cursor after `->`
    let items = complete_at(&backend, &uri, src, 8, 20).await;
    let names = method_names(&items);
    assert!(
        names.contains(&"getPrice"),
        "Expected getPrice in {:?}",
        names,
    );
    assert!(
        names.contains(&"getTitle"),
        "Expected getTitle in {:?}",
        names,
    );
}

// ─── Docblock callable return type annotation ───────────────────────────────

#[tokio::test]
async fn test_docblock_closure_return_type() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/docblock_closure.php").unwrap();

    let src = concat!(
        "<?php\n",
        "class Order {\n",
        "    public function getTotal(): float { return 0.0; }\n",
        "    public function getStatus(): string { return ''; }\n",
        "}\n",
        "class Service {\n",
        "    public function run(): void {\n",
        "        /** @var \\Closure(): Order $fn */\n",
        "        $fn = getCallback();\n",
        "        $fn()->\n",
        "    }\n",
        "}\n",
    );

    // Line 9: `        $fn()->`  cursor after `->`
    let items = complete_at(&backend, &uri, src, 9, 15).await;
    let names = method_names(&items);
    assert!(
        names.contains(&"getTotal"),
        "Expected getTotal in {:?}",
        names,
    );
    assert!(
        names.contains(&"getStatus"),
        "Expected getStatus in {:?}",
        names,
    );
}

#[tokio::test]
async fn test_docblock_callable_return_type() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/docblock_callable.php").unwrap();

    let src = concat!(
        "<?php\n",
        "class Response {\n",
        "    public function getBody(): string { return ''; }\n",
        "    public function getStatusCode(): int { return 200; }\n",
        "}\n",
        "class Handler {\n",
        "    /**\n",
        "     * @param callable(): Response $handler\n",
        "     */\n",
        "    public function process($handler): void {\n",
        "        $handler()->\n",
        "    }\n",
        "}\n",
    );

    // Line 10: `        $handler()->`  cursor after `->`
    let items = complete_at(&backend, &uri, src, 10, 21).await;
    let names = method_names(&items);
    assert!(
        names.contains(&"getBody"),
        "Expected getBody in {:?}",
        names,
    );
    assert!(
        names.contains(&"getStatusCode"),
        "Expected getStatusCode in {:?}",
        names,
    );
}

#[tokio::test]
async fn test_callable_with_params_return_type() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/callable_params.php").unwrap();

    let src = concat!(
        "<?php\n",
        "class Config {\n",
        "    public function get(string $key): string { return ''; }\n",
        "    public function set(string $key, $val): void {}\n",
        "}\n",
        "class App {\n",
        "    /**\n",
        "     * @param callable(string,int): Config $builder\n",
        "     */\n",
        "    public function init($builder): void {\n",
        "        $builder('test', 1)->\n",
        "    }\n",
        "}\n",
    );

    // Line 10: `        $builder('test', 1)->`  cursor after `->`
    let items = complete_at(&backend, &uri, src, 10, 30).await;
    let names = method_names(&items);
    assert!(names.contains(&"get"), "Expected get in {:?}", names,);
    assert!(names.contains(&"set"), "Expected set in {:?}", names,);
}

// ─── Variable assignment from closure invocation ────────────────────────────

#[tokio::test]
async fn test_variable_assigned_from_closure_invocation() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/var_closure_result.php").unwrap();

    let src = concat!(
        "<?php\n",
        "class Customer {\n",
        "    public function getId(): int { return 1; }\n",
        "    public function getFullName(): string { return ''; }\n",
        "}\n",
        "class Service {\n",
        "    public function run(): void {\n",
        "        $factory = function(): Customer { return new Customer(); };\n",
        "        $result = $factory();\n",
        "        $result->\n",
        "    }\n",
        "}\n",
    );

    // Line 9: `        $result->`  cursor after `->`
    let items = complete_at(&backend, &uri, src, 9, 17).await;
    let names = method_names(&items);
    assert!(names.contains(&"getId"), "Expected getId in {:?}", names,);
    assert!(
        names.contains(&"getFullName"),
        "Expected getFullName in {:?}",
        names,
    );
}

#[tokio::test]
async fn test_variable_assigned_from_docblock_callable_invocation() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/var_callable_result.php").unwrap();

    let src = concat!(
        "<?php\n",
        "class Item {\n",
        "    public function getWeight(): float { return 0.0; }\n",
        "}\n",
        "class Processor {\n",
        "    /**\n",
        "     * @param Closure(): Item $loader\n",
        "     */\n",
        "    public function handle($loader): void {\n",
        "        $item = $loader();\n",
        "        $item->\n",
        "    }\n",
        "}\n",
    );

    // Line 10: `        $item->`  cursor after `->`
    let items = complete_at(&backend, &uri, src, 10, 15).await;
    let names = method_names(&items);
    assert!(
        names.contains(&"getWeight"),
        "Expected getWeight in {:?}",
        names,
    );
}

// ─── Closure with `use` clause ──────────────────────────────────────────────

#[tokio::test]
async fn test_closure_with_use_clause_return_type() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/closure_use.php").unwrap();

    let src = concat!(
        "<?php\n",
        "class Logger {\n",
        "    public function log(string $msg): void {}\n",
        "    public function getLevel(): int { return 0; }\n",
        "}\n",
        "class Service {\n",
        "    public function run(): void {\n",
        "        $prefix = 'INFO';\n",
        "        $fn = function() use ($prefix): Logger { return new Logger(); };\n",
        "        $fn()->\n",
        "    }\n",
        "}\n",
    );

    // Line 9: `        $fn()->`  cursor after `->`
    let items = complete_at(&backend, &uri, src, 9, 15).await;
    let names = method_names(&items);
    assert!(names.contains(&"log"), "Expected log in {:?}", names,);
    assert!(
        names.contains(&"getLevel"),
        "Expected getLevel in {:?}",
        names,
    );
}

// ─── Top-level (outside class) ──────────────────────────────────────────────

#[tokio::test]
async fn test_closure_invocation_top_level() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/closure_toplevel.php").unwrap();

    let src = concat!(
        "<?php\n",
        "class Widget {\n",
        "    public function render(): string { return ''; }\n",
        "    public function hide(): void {}\n",
        "}\n",
        "$maker = function(): Widget { return new Widget(); };\n",
        "$maker()->\n",
    );

    // Line 6: `$maker()->`  cursor after `->`
    let items = complete_at(&backend, &uri, src, 6, 10).await;
    let names = method_names(&items);
    assert!(names.contains(&"render"), "Expected render in {:?}", names,);
    assert!(names.contains(&"hide"), "Expected hide in {:?}", names,);
}

// ─── Nullable return type ───────────────────────────────────────────────────

#[tokio::test]
async fn test_closure_nullable_return_type() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/closure_nullable.php").unwrap();

    let src = concat!(
        "<?php\n",
        "class Session {\n",
        "    public function getId(): string { return ''; }\n",
        "    public function destroy(): void {}\n",
        "}\n",
        "class App {\n",
        "    public function run(): void {\n",
        "        $getter = function(): ?Session { return null; };\n",
        "        $getter()->\n",
        "    }\n",
        "}\n",
    );

    // Line 8: `        $getter()->`  cursor after `->`
    let items = complete_at(&backend, &uri, src, 8, 19).await;
    let names = method_names(&items);
    assert!(names.contains(&"getId"), "Expected getId in {:?}", names,);
    assert!(
        names.contains(&"destroy"),
        "Expected destroy in {:?}",
        names,
    );
}

// ─── Chaining after callable invocation ─────────────────────────────────────

#[tokio::test]
async fn test_callable_invocation_chain() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/callable_chain.php").unwrap();

    let src = concat!(
        "<?php\n",
        "class Builder {\n",
        "    public function setName(string $n): self { return $this; }\n",
        "    public function build(): void {}\n",
        "}\n",
        "class Factory {\n",
        "    public function run(): void {\n",
        "        $make = function(): Builder { return new Builder(); };\n",
        "        $make()->setName('test')->\n",
        "    }\n",
        "}\n",
    );

    // Line 8: `        $make()->setName('test')->`  cursor after `->`
    let items = complete_at(&backend, &uri, src, 8, 37).await;
    let names = method_names(&items);
    assert!(
        names.contains(&"setName"),
        "Expected setName in {:?}",
        names,
    );
    assert!(names.contains(&"build"), "Expected build in {:?}", names,);
}

// ─── Cross-file PSR-4 ──────────────────────────────────────────────────────

#[tokio::test]
async fn test_callable_invocation_cross_file() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": {
                    "App\\": "src/"
                }
            }
        }"#,
        &[(
            "src/Models/Entity.php",
            concat!(
                "<?php\n",
                "namespace App\\Models;\n",
                "\n",
                "class Entity {\n",
                "    public function save(): bool { return true; }\n",
                "    public function delete(): void {}\n",
                "}\n",
            ),
        )],
    );

    // The "current" file references Entity via FQN in the closure return type
    let uri = Url::parse("file:///app.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Repo {\n",
        "    public function handle(): void {\n",
        "        $factory = function(): \\App\\Models\\Entity { return new \\App\\Models\\Entity(); };\n",
        "        $factory()->\n",
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

    // Cursor right after `$factory()->` on line 4
    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position {
                line: 4,
                character: 20,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let result = backend.completion(completion_params).await.unwrap();
    assert!(result.is_some(), "Completion should resolve $factory()->");

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let names: Vec<&str> = items
                .iter()
                .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
                .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
                .collect();
            assert!(names.contains(&"save"), "Expected save in {:?}", names,);
            assert!(names.contains(&"delete"), "Expected delete in {:?}", names,);
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── Docblock Closure with backslash prefix ─────────────────────────────────

#[tokio::test]
async fn test_docblock_fqn_closure_return_type() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/fqn_closure.php").unwrap();

    let src = concat!(
        "<?php\n",
        "class Database {\n",
        "    public function query(): void {}\n",
        "    public function disconnect(): void {}\n",
        "}\n",
        "class App {\n",
        "    /**\n",
        "     * @param \\Closure(): Database $connector\n",
        "     */\n",
        "    public function boot($connector): void {\n",
        "        $connector()->\n",
        "    }\n",
        "}\n",
    );

    // Line 10: `        $connector()->`  cursor after `->`
    let items = complete_at(&backend, &uri, src, 10, 22).await;
    let names = method_names(&items);
    assert!(names.contains(&"query"), "Expected query in {:?}", names,);
    assert!(
        names.contains(&"disconnect"),
        "Expected disconnect in {:?}",
        names,
    );
}

// ─── Properties on callable return type ─────────────────────────────────────

#[tokio::test]
async fn test_callable_return_type_properties() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/callable_props.php").unwrap();

    let src = concat!(
        "<?php\n",
        "class Point {\n",
        "    public float $x;\n",
        "    public float $y;\n",
        "    public function distanceTo(Point $other): float { return 0.0; }\n",
        "}\n",
        "class Geo {\n",
        "    public function run(): void {\n",
        "        /** @var callable(): Point $maker */\n",
        "        $maker = getMaker();\n",
        "        $maker()->\n",
        "    }\n",
        "}\n",
    );

    // Line 10: `        $maker()->`  cursor after `->`
    let items = complete_at(&backend, &uri, src, 10, 19).await;
    let names = method_names(&items);
    let props = property_names(&items);
    assert!(
        names.contains(&"distanceTo"),
        "Expected distanceTo in {:?}",
        names,
    );
    assert!(props.contains(&"x"), "Expected property x in {:?}", props,);
    assert!(props.contains(&"y"), "Expected property y in {:?}", props,);
}

// ─── Inline @var override for callable ──────────────────────────────────────

#[tokio::test]
async fn test_inline_var_callable_return_type() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/inline_var_callable.php").unwrap();

    let src = concat!(
        "<?php\n",
        "class Mailer {\n",
        "    public function send(): bool { return true; }\n",
        "    public function setSubject(string $s): self { return $this; }\n",
        "}\n",
        "class Notifier {\n",
        "    public function notify(): void {\n",
        "        /** @var Closure(): Mailer $fn */\n",
        "        $fn = getMailerFactory();\n",
        "        $fn()->\n",
        "    }\n",
        "}\n",
    );

    // Line 9: `        $fn()->`  cursor after `->`
    let items = complete_at(&backend, &uri, src, 9, 15).await;
    let names = method_names(&items);
    assert!(names.contains(&"send"), "Expected send in {:?}", names,);
    assert!(
        names.contains(&"setSubject"),
        "Expected setSubject in {:?}",
        names,
    );
}

// ── __invoke() return type resolution ──────────────────────────

/// `$f = new Invokable(); $f()->` resolves via __invoke() return type.
#[tokio::test]
async fn test_invoke_return_type_simple() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/invoke_simple.php").unwrap();

    let src = concat!(
        "<?php\n",
        "class Result { public function getValue(): string {} }\n",
        "class Invokable {\n",
        "    public function __invoke(): Result {}\n",
        "}\n",
        "$f = new Invokable();\n",
        "$f()->\n",
    );

    let items = complete_at(&backend, &uri, src, 6, 6).await;
    let methods = method_names(&items);
    assert!(
        methods.contains(&"getValue"),
        "Expected getValue from __invoke() return type, got: {methods:?}"
    );
}

/// `$f = new Invokable(); $f()->method()->` chains through __invoke().
#[tokio::test]
async fn test_invoke_return_type_chain() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/invoke_chain.php").unwrap();

    let src = concat!(
        "<?php\n",
        "class Builder {\n",
        "    public function build(): Product {}\n",
        "}\n",
        "class Product { public function getTitle(): string {} }\n",
        "class Factory {\n",
        "    public function __invoke(): Builder {}\n",
        "}\n",
        "$f = new Factory();\n",
        "$f()->build()->\n",
    );

    let items = complete_at(&backend, &uri, src, 9, 15).await;
    let methods = method_names(&items);
    assert!(
        methods.contains(&"getTitle"),
        "Expected getTitle from chained __invoke()->build(), got: {methods:?}"
    );
}

/// __invoke() on a variable assigned from a method returning an invokable.
#[tokio::test]
async fn test_invoke_return_type_from_method() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/invoke_method.php").unwrap();

    let src = concat!(
        "<?php\n",
        "class Output { public function render(): void {} }\n",
        "class Renderer {\n",
        "    public function __invoke(): Output {}\n",
        "}\n",
        "class App {\n",
        "    public function getRenderer(): Renderer {}\n",
        "}\n",
        "$app = new App();\n",
        "$r = $app->getRenderer();\n",
        "$r()->\n",
    );

    let items = complete_at(&backend, &uri, src, 10, 6).await;
    let methods = method_names(&items);
    assert!(
        methods.contains(&"render"),
        "Expected render from method-returned __invoke(), got: {methods:?}"
    );
}

/// Variable assigned an invokable via `new` at top level: `$h = new Handler(); $h()->...`
#[tokio::test]
async fn test_invoke_return_type_assigned_new() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/invoke_new.php").unwrap();

    let src = concat!(
        "<?php\n",
        "class Response { public function getStatus(): int {} }\n",
        "class Handler {\n",
        "    public function __invoke(): Response {}\n",
        "}\n",
        "$h = new Handler();\n",
        "$h()->\n",
    );

    let items = complete_at(&backend, &uri, src, 6, 6).await;
    let methods = method_names(&items);
    assert!(
        methods.contains(&"getStatus"),
        "Expected getStatus from $h = new Handler(); $h(), got: {methods:?}"
    );
}

/// __invoke() with docblock return type richer than native hint.
#[tokio::test]
async fn test_invoke_docblock_return_type() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/invoke_docblock.php").unwrap();

    let src = concat!(
        "<?php\n",
        "class Item { public function getLabel(): string {} }\n",
        "class Fetcher {\n",
        "    /** @return Item[] */\n",
        "    public function __invoke(): array {}\n",
        "}\n",
        "$f = new Fetcher();\n",
        "foreach ($f() as $item) {\n",
        "    $item->\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, src, 8, 11).await;
    let methods = method_names(&items);
    assert!(
        methods.contains(&"getLabel"),
        "Expected getLabel from __invoke() docblock @return Item[], got: {methods:?}"
    );
}

/// `($this->prop)()->` resolves through the property's __invoke() method.
#[tokio::test]
async fn test_invoke_parenthesized_property() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/invoke_paren_prop.php").unwrap();

    let src = concat!(
        "<?php\n",
        "class InvPen4 { public function write(): void {} }\n",
        "class MyInvoker4 {\n",
        "    public function __invoke(): InvPen4 {}\n",
        "}\n",
        "class InvApp4 {\n",
        "    private MyInvoker4 $invoker;\n",
        "    public function demo(): void {\n",
        "        ($this->invoker)()->\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, src, 8, 28).await;
    let methods = method_names(&items);
    assert!(
        methods.contains(&"write"),
        "Expected write from ($this->invoker)() __invoke(), got: {methods:?}"
    );
}

/// `($this->prop)()->` on a property annotated `@var callable(): T` reads
/// the return type out of the callable type itself — there is no class with
/// an `__invoke()` method to go through.
#[tokio::test]
async fn test_invoke_callable_typed_property() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/invoke_callable_prop.php").unwrap();

    let src = concat!(
        "<?php\n",
        "class InvScope5 { public function getType(): string {} }\n",
        "class InvApp5 {\n",
        "    /** @var callable(): InvScope5 */\n",
        "    private $factory;\n",
        "    public function demo(): void {\n",
        "        ($this->factory)()->\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, src, 6, 28).await;
    let methods = method_names(&items);
    assert!(
        methods.contains(&"getType"),
        "Expected getType from ($this->factory)() callable return type, got: {methods:?}"
    );
}
