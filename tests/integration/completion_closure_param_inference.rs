use crate::common::{create_psr4_workspace, create_test_backend};
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

/// Helper: open a document and trigger completion at the given line/column.
async fn complete_at(
    backend: &phpantom_lsp::Backend,
    uri: &Url,
    src: &str,
    line: u32,
    character: u32,
) -> Vec<CompletionItem> {
    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: src.to_string(),
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
        None => vec![],
    }
}

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

// ─── Arrow function: callable param inference from generic collection ───────

/// `$collection->map(fn($u) => $u->…)` — infer `$u` from the `map` method's
/// callable parameter type after generic substitution.
#[tokio::test]
async fn test_arrow_fn_param_inferred_from_generic_map() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/closure_infer_map.php").unwrap();

    let src = concat!(
        "<?php\n",
        "class User {\n",
        "    public function getName(): string { return ''; }\n",
        "    public function getEmail(): string { return ''; }\n",
        "}\n",
        "/**\n",
        " * @template TKey\n",
        " * @template TValue\n",
        " */\n",
        "class Collection {\n",
        "    /**\n",
        "     * @param callable(TValue): mixed $callback\n",
        "     * @return static\n",
        "     */\n",
        "    public function map(callable $callback): static {}\n",
        "}\n",
        "class UserService {\n",
        "    /** @return Collection<int, User> */\n",
        "    public function getUsers(): Collection {}\n",
        "    public function run(): void {\n",
        "        $users = $this->getUsers();\n",
        "        $users->map(fn($u) => $u->);\n",
        "    }\n",
        "}\n",
    );

    // Line 21: `        $users->map(fn($u) => $u->);`
    //                                          ^--- cursor after `->`
    let items = complete_at(&backend, &uri, src, 21, 34).await;
    let names = method_names(&items);
    assert!(
        names.contains(&"getName"),
        "Expected getName from inferred User type, got: {:?}",
        names,
    );
    assert!(
        names.contains(&"getEmail"),
        "Expected getEmail from inferred User type, got: {:?}",
        names,
    );
}

// ─── Closure: callable param inference from generic collection ──────────────

/// `$collection->each(function ($item) { $item->… })` — infer `$item` from
/// the `each` method's callable parameter type.
#[tokio::test]
async fn test_closure_param_inferred_from_generic_each() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/closure_infer_each.php").unwrap();

    let src = concat!(
        "<?php\n",
        "class Product {\n",
        "    public function getPrice(): float { return 0.0; }\n",
        "    public function getSku(): string { return ''; }\n",
        "}\n",
        "/**\n",
        " * @template TKey\n",
        " * @template TValue\n",
        " */\n",
        "class Collection {\n",
        "    /**\n",
        "     * @param callable(TValue): void $callback\n",
        "     * @return static\n",
        "     */\n",
        "    public function each(callable $callback): static {}\n",
        "}\n",
        "class ProductService {\n",
        "    /** @return Collection<int, Product> */\n",
        "    public function getProducts(): Collection {}\n",
        "    public function run(): void {\n",
        "        $products = $this->getProducts();\n",
        "        $products->each(function ($item) {\n",
        "            $item->\n",
        "        });\n",
        "    }\n",
        "}\n",
    );

    // Line 22: `            $item->`  cursor after `->`
    let items = complete_at(&backend, &uri, src, 22, 19).await;
    let names = method_names(&items);
    assert!(
        names.contains(&"getPrice"),
        "Expected getPrice from inferred Product type, got: {:?}",
        names,
    );
    assert!(
        names.contains(&"getSku"),
        "Expected getSku from inferred Product type, got: {:?}",
        names,
    );
}

// ─── Explicit type hint takes precedence over inference ─────────────────────

/// When a closure parameter has an explicit type hint, it should take
/// precedence over the inferred callable parameter type.
#[tokio::test]
async fn test_explicit_type_hint_takes_precedence() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/closure_explicit_wins.php").unwrap();

    let src = concat!(
        "<?php\n",
        "class Animal {\n",
        "    public function speak(): string { return ''; }\n",
        "}\n",
        "class Dog extends Animal {\n",
        "    public function fetch(): void {}\n",
        "}\n",
        "/**\n",
        " * @template TKey\n",
        " * @template TValue\n",
        " */\n",
        "class Collection {\n",
        "    /**\n",
        "     * @param callable(TValue): mixed $callback\n",
        "     * @return static\n",
        "     */\n",
        "    public function map(callable $callback): static {}\n",
        "}\n",
        "class Kennel {\n",
        "    /** @return Collection<int, Animal> */\n",
        "    public function getAnimals(): Collection {}\n",
        "    public function run(): void {\n",
        "        $animals = $this->getAnimals();\n",
        "        $animals->map(function (Dog $d) {\n",
        "            $d->\n",
        "        });\n",
        "    }\n",
        "}\n",
    );

    // Line 24: `            $d->`  cursor after `->`
    let items = complete_at(&backend, &uri, src, 24, 17).await;
    let names = method_names(&items);
    // Dog has `fetch` method, Animal does not
    assert!(
        names.contains(&"fetch"),
        "Explicit Dog type should win; expected fetch in {:?}",
        names,
    );
}

// ─── Non-callable parameter: no inference ───────────────────────────────────

/// When the method parameter at the closure's position is not a callable
/// type, no inference should be attempted and the param stays unresolved.
#[tokio::test]
async fn test_no_inference_for_non_callable_param() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/closure_no_infer.php").unwrap();

    let src = concat!(
        "<?php\n",
        "class Formatter {\n",
        "    public function format(string $value): string { return ''; }\n",
        "}\n",
        "class Service {\n",
        "    public function run(): void {\n",
        "        $f = new Formatter();\n",
        "        $f->format(fn($x) => $x->);\n",
        "    }\n",
        "}\n",
    );

    // Line 7: `        $f->format(fn($x) => $x->);`
    //                                        ^--- cursor after `->`
    let items = complete_at(&backend, &uri, src, 7, 34).await;
    let names = method_names(&items);
    // No type should be inferred for $x since `format` expects `string`, not callable
    assert!(
        names.is_empty() || !names.contains(&"format"),
        "Should not infer a type from non-callable param, got: {:?}",
        names,
    );
}

// ─── Concrete callable type (non-generic) ───────────────────────────────────

/// When the method parameter is `callable(SomeClass): void` (no generics
/// involved), the closure param should be inferred as `SomeClass`.
#[tokio::test]
async fn test_concrete_callable_param_inference() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/closure_concrete_callable.php").unwrap();

    let src = concat!(
        "<?php\n",
        "class Config {\n",
        "    public function get(string $key): string { return ''; }\n",
        "    public function set(string $key, string $val): void {}\n",
        "}\n",
        "class App {\n",
        "    /**\n",
        "     * @param callable(Config): void $callback\n",
        "     * @return void\n",
        "     */\n",
        "    public function configure(callable $callback): void {}\n",
        "}\n",
        "class Bootstrap {\n",
        "    public function run(): void {\n",
        "        $app = new App();\n",
        "        $app->configure(function ($cfg) {\n",
        "            $cfg->\n",
        "        });\n",
        "    }\n",
        "}\n",
    );

    // Line 16: `            $cfg->`  cursor after `->`
    let items = complete_at(&backend, &uri, src, 16, 18).await;
    let names = method_names(&items);
    assert!(
        names.contains(&"get"),
        "Expected get from inferred Config type, got: {:?}",
        names,
    );
    assert!(
        names.contains(&"set"),
        "Expected set from inferred Config type, got: {:?}",
        names,
    );
}

// ─── Arrow function with properties ─────────────────────────────────────────

/// Inferred type should resolve both methods and properties.
#[tokio::test]
async fn test_inferred_type_resolves_properties() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/closure_infer_props.php").unwrap();

    let src = concat!(
        "<?php\n",
        "class Point {\n",
        "    public float $x;\n",
        "    public float $y;\n",
        "    public function distanceTo(Point $other): float { return 0.0; }\n",
        "}\n",
        "/**\n",
        " * @template TKey\n",
        " * @template TValue\n",
        " */\n",
        "class Collection {\n",
        "    /**\n",
        "     * @param callable(TValue): mixed $callback\n",
        "     * @return static\n",
        "     */\n",
        "    public function map(callable $callback): static {}\n",
        "}\n",
        "class Geometry {\n",
        "    /** @return Collection<int, Point> */\n",
        "    public function getPoints(): Collection {}\n",
        "    public function run(): void {\n",
        "        $points = $this->getPoints();\n",
        "        $points->map(fn($p) => $p->);\n",
        "    }\n",
        "}\n",
    );

    // Line 22: `        $points->map(fn($p) => $p->);`
    //                                           ^--- cursor after `->`
    let items = complete_at(&backend, &uri, src, 22, 35).await;
    let props = property_names(&items);
    assert!(
        props.contains(&"x"),
        "Expected property x from inferred Point type, got: {:?}",
        props,
    );
    assert!(
        props.contains(&"y"),
        "Expected property y from inferred Point type, got: {:?}",
        props,
    );
    let methods = method_names(&items);
    assert!(
        methods.contains(&"distanceTo"),
        "Expected distanceTo from inferred Point type, got: {:?}",
        methods,
    );
}

// ─── Static method call with callable param ─────────────────────────────────

/// `MyClass::process(function ($item) { $item->… })` — infer from static
/// method's callable parameter type.
#[tokio::test]
async fn test_static_method_callable_param_inference() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/closure_static_infer.php").unwrap();

    let src = concat!(
        "<?php\n",
        "class Order {\n",
        "    public function getTotal(): float { return 0.0; }\n",
        "    public function getStatus(): string { return ''; }\n",
        "}\n",
        "class Processor {\n",
        "    /**\n",
        "     * @param callable(Order): void $handler\n",
        "     * @return void\n",
        "     */\n",
        "    public static function handle(callable $handler): void {}\n",
        "}\n",
        "class Runner {\n",
        "    public function run(): void {\n",
        "        Processor::handle(function ($order) {\n",
        "            $order->\n",
        "        });\n",
        "    }\n",
        "}\n",
    );

    // Line 15: `            $order->`  cursor after `->`
    let items = complete_at(&backend, &uri, src, 15, 21).await;
    let names = method_names(&items);
    assert!(
        names.contains(&"getTotal"),
        "Expected getTotal from inferred Order type, got: {:?}",
        names,
    );
    assert!(
        names.contains(&"getStatus"),
        "Expected getStatus from inferred Order type, got: {:?}",
        names,
    );
}

// ─── Multiple callable parameters ──────────────────────────────────────────

/// When a method has multiple parameters and the callable is not at
/// position 0, the inference should use the correct parameter index.
#[tokio::test]
async fn test_callable_at_non_zero_position() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/closure_second_arg.php").unwrap();

    let src = concat!(
        "<?php\n",
        "class Item {\n",
        "    public function getWeight(): float { return 0.0; }\n",
        "}\n",
        "/**\n",
        " * @template TKey\n",
        " * @template TValue\n",
        " */\n",
        "class Collection {\n",
        "    /**\n",
        "     * @param int $size\n",
        "     * @param callable(TValue): void $callback\n",
        "     * @return void\n",
        "     */\n",
        "    public function chunk(int $size, callable $callback): void {}\n",
        "}\n",
        "class Warehouse {\n",
        "    /** @return Collection<int, Item> */\n",
        "    public function getItems(): Collection {}\n",
        "    public function run(): void {\n",
        "        $items = $this->getItems();\n",
        "        $items->chunk(100, function ($item) {\n",
        "            $item->\n",
        "        });\n",
        "    }\n",
        "}\n",
    );

    // Line 22: `            $item->`  cursor after `->`
    let items = complete_at(&backend, &uri, src, 22, 19).await;
    let names = method_names(&items);
    assert!(
        names.contains(&"getWeight"),
        "Expected getWeight from inferred Item type at arg position 1, got: {:?}",
        names,
    );
}

// ─── Closure parameter with Closure() type ──────────────────────────────────

/// When the method parameter uses `Closure(Type): ReturnType` syntax.
#[tokio::test]
async fn test_closure_type_syntax_inference() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/closure_type_syntax.php").unwrap();

    let src = concat!(
        "<?php\n",
        "class Logger {\n",
        "    public function info(string $msg): void {}\n",
        "    public function error(string $msg): void {}\n",
        "}\n",
        "class Pipeline {\n",
        "    /**\n",
        "     * @param Closure(Logger): void $step\n",
        "     * @return static\n",
        "     */\n",
        "    public function pipe(\\Closure $step): static {}\n",
        "}\n",
        "class Runner {\n",
        "    public function run(): void {\n",
        "        $pipeline = new Pipeline();\n",
        "        $pipeline->pipe(fn($log) => $log->);\n",
        "    }\n",
        "}\n",
    );

    // Line 15: `        $pipeline->pipe(fn($log) => $log->);`
    //                                                ^--- cursor after `->`
    let items = complete_at(&backend, &uri, src, 15, 42).await;
    let names = method_names(&items);
    assert!(
        names.contains(&"info"),
        "Expected info from inferred Logger type, got: {:?}",
        names,
    );
    assert!(
        names.contains(&"error"),
        "Expected error from inferred Logger type, got: {:?}",
        names,
    );
}

// ─── $this-> receiver ───────────────────────────────────────────────────────

/// Callable inference when the method is called on `$this->`.
#[tokio::test]
async fn test_inference_on_this_method_call() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/closure_this_call.php").unwrap();

    let src = concat!(
        "<?php\n",
        "class Task {\n",
        "    public function execute(): bool { return true; }\n",
        "    public function getName(): string { return ''; }\n",
        "}\n",
        "class TaskRunner {\n",
        "    /**\n",
        "     * @param callable(Task): void $fn\n",
        "     * @return void\n",
        "     */\n",
        "    public function runWith(callable $fn): void {}\n",
        "    public function run(): void {\n",
        "        $this->runWith(function ($task) {\n",
        "            $task->\n",
        "        });\n",
        "    }\n",
        "}\n",
    );

    // Line 13: `            $task->`  cursor after `->`
    let items = complete_at(&backend, &uri, src, 13, 19).await;
    let names = method_names(&items);
    assert!(
        names.contains(&"execute"),
        "Expected execute from inferred Task type, got: {:?}",
        names,
    );
    assert!(
        names.contains(&"getName"),
        "Expected getName from inferred Task type, got: {:?}",
        names,
    );
}

// ─── $this / static in callable param types resolve to receiver class ───────

/// When a method signature uses `$this` in a callable parameter type (e.g.
/// `callable($this, mixed): $this`), the inferred closure parameter should
/// resolve to the receiver class, not the class the user is editing.
#[tokio::test]
async fn test_this_in_callable_param_resolves_to_receiver() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/closure_this_receiver.php").unwrap();

    let src = concat!(
        "<?php\n",
        "trait Conditionable {\n",
        "    /**\n",
        "     * @param callable($this, mixed): $this $callback\n",
        "     * @return $this\n",
        "     */\n",
        "    public function when(bool $condition, callable $callback): static {}\n",
        "}\n",
        "class Builder {\n",
        "    use Conditionable;\n",
        "    public function where(string $col, mixed $val): static { return $this; }\n",
        "    public function orderBy(string $col): static { return $this; }\n",
        "}\n",
        "class UserController {\n",
        "    public function index(): void {\n",
        "        $builder = new Builder();\n",
        "        $builder->when(true, function ($query) {\n",
        "            $query->\n",
        "        });\n",
        "    }\n",
        "}\n",
    );

    // Line 17: `            $query->`  cursor after `->`
    let items = complete_at(&backend, &uri, src, 17, 21).await;
    let names = method_names(&items);
    assert!(
        names.contains(&"where"),
        "Expected 'where' from Builder (receiver), got: {:?}",
        names,
    );
    assert!(
        names.contains(&"orderBy"),
        "Expected 'orderBy' from Builder (receiver), got: {:?}",
        names,
    );
}

/// Same scenario but with `static` instead of `$this` in the callable param type.
#[tokio::test]
async fn test_static_in_callable_param_resolves_to_receiver() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/closure_static_receiver.php").unwrap();

    let src = concat!(
        "<?php\n",
        "class Pipeline {\n",
        "    /**\n",
        "     * @param callable(static): void $callback\n",
        "     * @return static\n",
        "     */\n",
        "    public function tap(callable $callback): static {}\n",
        "    public function send(mixed $data): static { return $this; }\n",
        "    public function through(array $pipes): static { return $this; }\n",
        "}\n",
        "class SomeService {\n",
        "    public function run(): void {\n",
        "        $pipeline = new Pipeline();\n",
        "        $pipeline->tap(function ($p) {\n",
        "            $p->\n",
        "        });\n",
        "    }\n",
        "}\n",
    );

    // Line 14: `            $p->`  cursor after `->`
    let items = complete_at(&backend, &uri, src, 14, 17).await;
    let names = method_names(&items);
    assert!(
        names.contains(&"send"),
        "Expected 'send' from Pipeline (receiver), got: {:?}",
        names,
    );
    assert!(
        names.contains(&"through"),
        "Expected 'through' from Pipeline (receiver), got: {:?}",
        names,
    );
}

/// Same as `test_static_in_callable_param_resolves_to_receiver` but uses the
/// exact Laravel `where()` param type: `(\Closure(static): mixed)|string|array`.
///
/// This is a parenthesized `Closure(…)` (not bare `callable(…)`) in a union
/// with non-callable types and the callable return type is `mixed` (not `void`).
/// The native PHP type hint is absent (just `$column` with no type).
#[tokio::test]
async fn test_laravel_where_closure_param_resolves_to_receiver() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/closure_laravel_where.php").unwrap();

    let src = concat!(
        "<?php\n",
        "class Builder {\n",
        "    /**\n",
        "     * @param (\\Closure(static): mixed)|string|array $column\n",
        "     * @return static\n",
        "     */\n",
        "    public function where($column, $operator = null, $value = null): static { return $this; }\n",
        "    public function whereNotIn(string $col, array $vals): static { return $this; }\n",
        "    public function orWhere(string $col, mixed $val = null): static { return $this; }\n",
        "}\n",
        "class SomeService {\n",
        "    public function run(): void {\n",
        "        $builder = new Builder();\n",
        "        $builder->where(function ($q) {\n",
        "            $q->\n",
        "        });\n",
        "    }\n",
        "}\n",
    );

    // Line 14: `            $q->`  cursor after `->`
    let items = complete_at(&backend, &uri, src, 14, 17).await;
    let names = method_names(&items);
    assert!(
        names.contains(&"whereNotIn"),
        "Expected 'whereNotIn' from Builder via (\\Closure(static): mixed)|string|array, got: {:?}",
        names,
    );
    assert!(
        names.contains(&"orWhere"),
        "Expected 'orWhere' from Builder via (\\Closure(static): mixed)|string|array, got: {:?}",
        names,
    );
}

/// `$this` in a callable param type on a static method call should also
/// resolve to the receiver class.
#[tokio::test]
async fn test_this_in_callable_param_static_call_resolves_to_receiver() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/closure_this_static_call.php").unwrap();

    let src = concat!(
        "<?php\n",
        "class QueryBuilder {\n",
        "    /**\n",
        "     * @param callable($this): void $callback\n",
        "     * @return static\n",
        "     */\n",
        "    public static function create(callable $callback): static {}\n",
        "    public function limit(int $n): static { return $this; }\n",
        "    public function offset(int $n): static { return $this; }\n",
        "}\n",
        "class Controller {\n",
        "    public function index(): void {\n",
        "        QueryBuilder::create(function ($qb) {\n",
        "            $qb->\n",
        "        });\n",
        "    }\n",
        "}\n",
    );

    // Line 13: `            $qb->`  cursor after `->`
    let items = complete_at(&backend, &uri, src, 13, 18).await;
    let names = method_names(&items);
    assert!(
        names.contains(&"limit"),
        "Expected 'limit' from QueryBuilder (static receiver), got: {:?}",
        names,
    );
    assert!(
        names.contains(&"offset"),
        "Expected 'offset' from QueryBuilder (static receiver), got: {:?}",
        names,
    );
}

/// Arrow function variant: `$this` in callable param resolves to receiver.
#[tokio::test]
async fn test_this_in_callable_param_arrow_fn() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/closure_this_arrow.php").unwrap();

    let src = concat!(
        "<?php\n",
        "class Filterable {\n",
        "    /**\n",
        "     * @param callable($this): bool $predicate\n",
        "     * @return static\n",
        "     */\n",
        "    public function filter(callable $predicate): static {}\n",
        "    public function isActive(): bool { return true; }\n",
        "    public function getName(): string { return ''; }\n",
        "}\n",
        "class App {\n",
        "    public function run(): void {\n",
        "        $f = new Filterable();\n",
        "        $f->filter(fn($item) => $item->);\n",
        "    }\n",
        "}\n",
    );

    // Line 13: `        $f->filter(fn($item) => $item->);`
    //                                             ^--- cursor after `->`
    let items = complete_at(&backend, &uri, src, 13, 39).await;
    let names = method_names(&items);
    assert!(
        names.contains(&"isActive"),
        "Expected 'isActive' from Filterable (receiver), got: {:?}",
        names,
    );
    assert!(
        names.contains(&"getName"),
        "Expected 'getName' from Filterable (receiver), got: {:?}",
        names,
    );
}

/// When a closure parameter has an explicit bare type hint (e.g.
/// `Collection $customers`) and the callable signature infers a more
/// specific generic form (e.g. `Collection<int, Customer>`), the inferred
/// type should be used so that foreach iteration resolves the element type.
///
/// Reproduces: `Customer::chunk(10, function (Collection $customers) { foreach ($customers as $customer) { $customer->… } })`
#[tokio::test]
async fn test_explicit_bare_hint_uses_inferred_generics_for_foreach() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/closure_explicit_generic_foreach.php").unwrap();

    let src = concat!(
        "<?php\n",
        "class Customer {\n",
        "    public function isActiveMember(): bool { return true; }\n",
        "    public function getEmail(): string { return ''; }\n",
        "}\n",
        "/**\n",
        " * @template TKey of array-key\n",
        " * @template TValue\n",
        " * @implements IteratorAggregate<TKey, TValue>\n",
        " */\n",
        "class Collection {\n",
        "    /** @return TValue|null */\n",
        "    public function first(): mixed { return null; }\n",
        "    /** @return int */\n",
        "    public function count(): int { return 0; }\n",
        "}\n",
        "/**\n",
        " * @template TModel\n",
        " */\n",
        "class Builder {\n",
        "    /**\n",
        "     * @param callable(Collection<int, TModel>, int): mixed $callback\n",
        "     * @return bool\n",
        "     */\n",
        "    public function chunk(int $count, callable $callback): bool { return true; }\n",
        "    /** @return static */\n",
        "    public function where(string $col, mixed $val = null): static { return $this; }\n",
        "}\n",
        "class Service {\n",
        "    /** @return Builder<Customer> */\n",
        "    public function query(): Builder { return new Builder(); }\n",
        "    public function run(): void {\n",
        "        $this->query()->chunk(10, function (Collection $customers): void {\n",
        "            foreach ($customers as $customer) {\n",
        "                $customer->\n",
        "            }\n",
        "        });\n",
        "    }\n",
        "}\n",
    );

    // Line 34: `                $customer->` inside the foreach body.
    let items = complete_at(&backend, &uri, src, 34, 28).await;
    let names = method_names(&items);
    assert!(
        names.contains(&"isActiveMember"),
        "Expected isActiveMember from Customer via foreach over Collection<int, Customer>, got: {:?}",
        names,
    );
    assert!(
        names.contains(&"getEmail"),
        "Expected getEmail from Customer via foreach over Collection<int, Customer>, got: {:?}",
        names,
    );
}

/// Simpler variant: static call chain where the closure param is the
/// direct target (no foreach indirection). Verifies that
/// `infer_callable_params_from_receiver` works when the receiver is a
/// static method call result.
#[tokio::test]
async fn test_static_call_chain_closure_param_direct() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/closure_static_chain_direct.php").unwrap();

    let src = concat!(
        "<?php\n",
        "class Customer {\n",
        "    public function isActiveMember(): bool { return true; }\n",
        "    public function getEmail(): string { return ''; }\n",
        "}\n",
        "/**\n",
        " * @template TKey of array-key\n",
        " * @template TValue\n",
        " * @implements IteratorAggregate<TKey, TValue>\n",
        " */\n",
        "class Collection {\n",
        "    /** @return TValue|null */\n",
        "    public function first(): mixed { return null; }\n",
        "    /** @return int */\n",
        "    public function count(): int { return 0; }\n",
        "}\n",
        "/**\n",
        " * @template TModel\n",
        " */\n",
        "class Builder {\n",
        "    /**\n",
        "     * @param callable(Collection<int, TModel>): mixed $callback\n",
        "     * @return bool\n",
        "     */\n",
        "    public function each(callable $callback): bool { return true; }\n",
        "    /** @return static */\n",
        "    public function where(string $col, mixed $val = null): static { return $this; }\n",
        "}\n",
        "class Customer2 {\n",
        "    /** @return Builder<Customer> */\n",
        "    public static function where(string $col, mixed $val = null): Builder { return new Builder(); }\n",
        "}\n",
        "class Service {\n",
        "    public function run(): void {\n",
        "        Customer2::where('active', true)->each(function ($items) {\n",
        "            $items->\n",
        "        });\n",
        "    }\n",
        "}\n",
    );

    // Line 35: `            $items->` — $items is closure param inferred
    // from `each(callable(Collection<int, Customer>): mixed)`.
    let items = complete_at(&backend, &uri, src, 35, 20).await;
    let names = method_names(&items);
    assert!(
        names.contains(&"first"),
        "Expected 'first' from Collection on closure param inferred via static call chain, got: {:?}",
        names,
    );
    assert!(
        names.contains(&"count"),
        "Expected 'count' from Collection on closure param inferred via static call chain, got: {:?}",
        names,
    );
}

/// Same as the previous test but the receiver is a **static** method call
/// chain (`Model::where(…)->chunk(…)`) instead of an instance method call.
/// This exercises `infer_callable_params_from_receiver` where the object
/// expression is a static call that must be resolved through the call-return
/// pipeline before the `chunk` method's callable signature can be inspected.
///
/// Reproduces the example.php pattern:
///   `BlogAuthor::where('active', true)->chunk(100, function (Collection $authors) { … })`
#[tokio::test]
async fn test_explicit_bare_hint_via_static_call_chain_for_foreach() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/closure_static_chain_foreach.php").unwrap();

    let src = concat!(
        "<?php\n",
        "class Customer {\n",
        "    public function isActiveMember(): bool { return true; }\n",
        "    public function getEmail(): string { return ''; }\n",
        "}\n",
        "/**\n",
        " * @template TKey of array-key\n",
        " * @template TValue\n",
        " * @implements IteratorAggregate<TKey, TValue>\n",
        " */\n",
        "class Collection {\n",
        "    /** @return TValue|null */\n",
        "    public function first(): mixed { return null; }\n",
        "    /** @return int */\n",
        "    public function count(): int { return 0; }\n",
        "}\n",
        "/**\n",
        " * @template TModel\n",
        " */\n",
        "class Builder {\n",
        "    /**\n",
        "     * @param callable(Collection<int, TModel>, int): mixed $callback\n",
        "     * @return bool\n",
        "     */\n",
        "    public function chunk(int $count, callable $callback): bool { return true; }\n",
        "    /** @return static */\n",
        "    public function where(string $col, mixed $val = null): static { return $this; }\n",
        "}\n",
        "/**\n",
        " * @extends Builder<Customer>\n",
        " */\n",
        "class CustomerBuilder extends Builder {\n",
        "}\n",
        "class Customer2 {\n",
        "    /** @return Builder<Customer> */\n",
        "    public static function where(string $col, mixed $val = null): Builder { return new Builder(); }\n",
        "}\n",
        "class Service {\n",
        "    public function run(): void {\n",
        "        Customer2::where('active', true)->chunk(10, function (Collection $customers): void {\n",
        "            foreach ($customers as $customer) {\n",
        "                $customer->\n",
        "            }\n",
        "        });\n",
        "    }\n",
        "}\n",
    );

    // Line 41: `                $customer->` inside the foreach body.
    let items = complete_at(&backend, &uri, src, 41, 28).await;
    let names = method_names(&items);
    assert!(
        names.contains(&"isActiveMember"),
        "Expected isActiveMember from Customer via static call chain + foreach over Collection<int, Customer>, got: {:?}",
        names,
    );
    assert!(
        names.contains(&"getEmail"),
        "Expected getEmail from Customer via static call chain + foreach over Collection<int, Customer>, got: {:?}",
        names,
    );
}

/// Full integration test mimicking example.php's structure: namespaced
/// Eloquent stubs with `BuildsQueries` trait, `Model::where()` forwarded
/// via builder virtual members, `chunk` callable param inference through
/// foreach with an explicit bare `Collection` type hint.
///
/// This is the most realistic reproduction of the example.php pattern:
///   `BlogAuthor::where('active', true)->chunk(100, function (Collection $authors) {
///       foreach ($authors as $author) { $author->posts(); }
///   })`
#[tokio::test]
async fn test_inline_stubs_chunk_closure_foreach_resolution() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/inline_stubs_chunk_foreach.php").unwrap();

    // Mimics example.php: model in a Demo namespace, Illuminate stubs in
    // separate namespace blocks in the same file.
    let src = concat!(
        "<?php\n",
        "namespace Demo {\n",
        "\n",
        "class BlogAuthor extends \\Illuminate\\Database\\Eloquent\\Model\n",
        "{\n",
        "    public function posts(): mixed { return null; }\n",
        "    public function getName(): string { return ''; }\n",
        "    public function demo(): void\n",
        "    {\n",
        "        BlogAuthor::where('active', true)->chunk(100, function (\\Illuminate\\Support\\Collection $authors) {\n",
        "            foreach ($authors as $author) {\n",
        "                $author->\n",
        "            }\n",
        "        });\n",
        "    }\n",
        "}\n",
        "\n",
        "} // end namespace Demo\n",
        "\n",
        "namespace Illuminate\\Database\\Eloquent {\n",
        "    abstract class Model {\n",
        "        /** @return \\Illuminate\\Database\\Eloquent\\Builder<static> */\n",
        "        public static function query() {}\n",
        "    }\n",
        "\n",
        "    /**\n",
        "     * @template TModel of \\Illuminate\\Database\\Eloquent\\Model\n",
        "     * @mixin \\Illuminate\\Database\\Query\\Builder\n",
        "     */\n",
        "    class Builder implements \\Illuminate\\Contracts\\Database\\Eloquent\\Builder {\n",
        "        /** @use \\Illuminate\\Database\\Concerns\\BuildsQueries<TModel> */\n",
        "        use \\Illuminate\\Database\\Concerns\\BuildsQueries;\n",
        "\n",
        "        /** @return $this */\n",
        "        public function where($column, $operator = null, $value = null) {}\n",
        "\n",
        "        /** @return \\Illuminate\\Database\\Eloquent\\Collection<int, TModel> */\n",
        "        public function get($columns = ['*']) { return new Collection(); }\n",
        "    }\n",
        "\n",
        "    /**\n",
        "     * @template TKey of array-key\n",
        "     * @template TModel of \\Illuminate\\Database\\Eloquent\\Model\n",
        "     */\n",
        "    class Collection {\n",
        "        /** @return TModel|null */\n",
        "        public function first(): mixed { return null; }\n",
        "        public function count(): int { return 0; }\n",
        "    }\n",
        "}\n",
        "\n",
        "namespace Illuminate\\Contracts\\Database\\Eloquent {\n",
        "    interface Builder {}\n",
        "}\n",
        "\n",
        "namespace Illuminate\\Database\\Eloquent\\Relations {\n",
        "    class HasMany {}\n",
        "    class HasOne {}\n",
        "    class BelongsTo {}\n",
        "    class BelongsToMany {}\n",
        "    class MorphOne {}\n",
        "    class MorphMany {}\n",
        "    class MorphTo {}\n",
        "    class MorphToMany {}\n",
        "    class HasManyThrough {}\n",
        "    class HasOneThrough {}\n",
        "}\n",
        "\n",
        "namespace Illuminate\\Database\\Concerns {\n",
        "    /**\n",
        "     * @template TValue\n",
        "     */\n",
        "    trait BuildsQueries {\n",
        "        /** @return TValue|null */\n",
        "        public function first($columns = ['*']) { return null; }\n",
        "\n",
        "        /**\n",
        "         * @param  callable(\\Illuminate\\Support\\Collection<int, TValue>, int): mixed  $callback\n",
        "         * @return bool\n",
        "         */\n",
        "        public function chunk(int $count, callable $callback): bool { return true; }\n",
        "    }\n",
        "}\n",
        "\n",
        "namespace Illuminate\\Database\\Query {\n",
        "    class Builder {\n",
        "        /** @return $this */\n",
        "        public function orderBy($column, $direction = 'asc') { return $this; }\n",
        "    }\n",
        "}\n",
        "\n",
        "namespace Illuminate\\Support {\n",
        "    /**\n",
        "     * @template TKey of array-key\n",
        "     * @template TValue\n",
        "     * @implements \\IteratorAggregate<TKey, TValue>\n",
        "     */\n",
        "    class Collection {\n",
        "        /** @return TValue|null */\n",
        "        public function first(): mixed { return null; }\n",
        "        /** @return int */\n",
        "        public function count(): int { return 0; }\n",
        "    }\n",
        "}\n",
    );

    // BlogAuthor::where(...)  → Builder<BlogAuthor> (via builder forwarding)
    // ->chunk(100, fn(Collection $authors) => ...)
    //   chunk's callable signature: callable(Collection<int, TModel>, int): mixed
    //   with TModel=BlogAuthor → callable(Collection<int, BlogAuthor>, int): mixed
    // Explicit bare "Collection $authors" inherits inferred Collection<int, BlogAuthor>
    // foreach ($authors as $author) → $author is BlogAuthor
    //
    // Line 11: `                $author->` inside the foreach body.
    let items = complete_at(&backend, &uri, src, 11, 25).await;
    let names = method_names(&items);
    assert!(
        names.contains(&"posts"),
        "Expected 'posts' from BlogAuthor via chunk closure + foreach over Collection<int, BlogAuthor>, got: {:?}",
        names,
    );
    assert!(
        names.contains(&"getName"),
        "Expected 'getName' from BlogAuthor via chunk closure + foreach over Collection<int, BlogAuthor>, got: {:?}",
        names,
    );
}

/// Exact reproduction of example.php's structure: the completion site is
/// inside `ClosureParamInferenceDemo::demo()` (a *different* class from
/// `BlogAuthor`), and the closure param uses a bare `Collection` hint
/// (not fully-qualified `\Illuminate\Support\Collection`).
///
/// This catches two differences from `test_inline_stubs_chunk_closure_foreach_resolution`:
///   1. `current_class` is `ClosureParamInferenceDemo`, not `BlogAuthor`
///   2. The explicit hint is the bare name `Collection`, relying on
///      inference to provide the FQN with generic args
#[tokio::test]
async fn test_example_php_exact_layout_chunk_foreach() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/example_php_layout.php").unwrap();

    let src = concat!(
        "<?php\n",                                                            // 0
        "namespace Demo {\n",                                                 // 1
        "\n",                                                                 // 2
        "class BlogAuthor extends \\Illuminate\\Database\\Eloquent\\Model\n", // 3
        "{\n",                                                                // 4
        "    public function posts(): mixed { return null; }\n",              // 5
        "    public function getName(): string { return ''; }\n",             // 6
        "}\n",                                                                // 7
        "\n",                                                                 // 8
        "class ClosureParamInferenceDemo\n",                                  // 9
        "{\n",                                                                // 10
        "    public function demo(): void\n",                                 // 11
        "    {\n",                                                            // 12
        "        BlogAuthor::where('active', true)->chunk(100, function (Collection $authors) {\n", // 13
        "            foreach ($authors as $author) {\n", // 14
        "                $author->\n",                   // 15
        "            }\n",                               // 16
        "        });\n",                                 // 17
        "    }\n",                                       // 18
        "}\n",                                           // 19
        "\n",                                            // 20
        "} // end namespace Demo\n",                     // 21
        "\n",                                            // 22
        "namespace Illuminate\\Database\\Eloquent {\n",  // 23
        "    abstract class Model {\n",                  // 24
        "        /** @return \\Illuminate\\Database\\Eloquent\\Builder<static> */\n", // 25
        "        public static function query() {}\n",   // 26
        "    }\n",                                       // 27
        "\n",                                            // 28
        "    /**\n",                                     // 29
        "     * @template TModel of \\Illuminate\\Database\\Eloquent\\Model\n", // 30
        "     * @mixin \\Illuminate\\Database\\Query\\Builder\n", // 31
        "     */\n",                                     // 32
        "    class Builder implements \\Illuminate\\Contracts\\Database\\Eloquent\\Builder {\n", // 33
        "        /** @use \\Illuminate\\Database\\Concerns\\BuildsQueries<TModel> */\n", // 34
        "        use \\Illuminate\\Database\\Concerns\\BuildsQueries;\n",                // 35
        "\n",                                                                            // 36
        "        /** @return $this */\n",                                                // 37
        "        public function where($column, $operator = null, $value = null) {}\n",  // 38
        "\n",                                                                            // 39
        "        /** @return \\Illuminate\\Database\\Eloquent\\Collection<int, TModel> */\n", // 40
        "        public function get($columns = ['*']) { return new Collection(); }\n",  // 41
        "    }\n",                                                                       // 42
        "\n",                                                                            // 43
        "    /**\n",                                                                     // 44
        "     * @template TKey of array-key\n",                                          // 45
        "     * @template TModel of \\Illuminate\\Database\\Eloquent\\Model\n",          // 46
        "     */\n",                                                                     // 47
        "    class Collection {\n",                                                      // 48
        "        /** @return TModel|null */\n",                                          // 49
        "        public function first(): mixed { return null; }\n",                     // 50
        "        public function count(): int { return 0; }\n",                          // 51
        "    }\n",                                                                       // 52
        "}\n",                                                                           // 53
        "\n",                                                                            // 54
        "namespace Illuminate\\Contracts\\Database\\Eloquent {\n",                       // 55
        "    interface Builder {}\n",                                                    // 56
        "}\n",                                                                           // 57
        "\n",                                                                            // 58
        "namespace Illuminate\\Database\\Eloquent\\Relations {\n",                       // 59
        "    class HasMany {}\n",                                                        // 60
        "    class HasOne {}\n",                                                         // 61
        "    class BelongsTo {}\n",                                                      // 62
        "    class BelongsToMany {}\n",                                                  // 63
        "    class MorphOne {}\n",                                                       // 64
        "    class MorphMany {}\n",                                                      // 65
        "    class MorphTo {}\n",                                                        // 66
        "    class MorphToMany {}\n",                                                    // 67
        "    class HasManyThrough {}\n",                                                 // 68
        "    class HasOneThrough {}\n",                                                  // 69
        "}\n",                                                                           // 70
        "\n",                                                                            // 71
        "namespace Illuminate\\Database\\Concerns {\n",                                  // 72
        "    /**\n",                                                                     // 73
        "     * @template TValue\n",                                                     // 74
        "     */\n",                                                                     // 75
        "    trait BuildsQueries {\n",                                                   // 76
        "        /** @return TValue|null */\n",                                          // 77
        "        public function first($columns = ['*']) { return null; }\n",            // 78
        "\n",                                                                            // 79
        "        /**\n",                                                                 // 80
        "         * @param  callable(\\Illuminate\\Support\\Collection<int, TValue>, int): mixed  $callback\n", // 81
        "         * @return bool\n", // 82
        "         */\n",             // 83
        "        public function chunk(int $count, callable $callback): bool { return true; }\n", // 84
        "    }\n",                                   // 85
        "}\n",                                       // 86
        "\n",                                        // 87
        "namespace Illuminate\\Database\\Query {\n", // 88
        "    class Builder {\n",                     // 89
        "        /** @return $this */\n",            // 90
        "        public function orderBy($column, $direction = 'asc') { return $this; }\n", // 91
        "    }\n",                                   // 92
        "}\n",                                       // 93
        "\n",                                        // 94
        "namespace Illuminate\\Support {\n",         // 95
        "    /**\n",                                 // 96
        "     * @template TKey of array-key\n",      // 97
        "     * @template TValue\n",                 // 98
        "     * @implements \\IteratorAggregate<TKey, TValue>\n", // 99
        "     */\n",                                 // 100
        "    class Collection {\n",                  // 101
        "        /** @return TValue|null */\n",      // 102
        "        public function first(): mixed { return null; }\n", // 103
        "        /** @return int */\n",              // 104
        "        public function count(): int { return 0; }\n", // 105
        "    }\n",                                   // 106
        "}\n",                                       // 107
    );

    // Line 15: `                $author->` inside the foreach body.
    // BlogAuthor::where(...) → Builder<BlogAuthor>
    // ->chunk(100, fn(Collection $authors) => ...)
    //   chunk callable sig: callable(Collection<int, TValue>, int): mixed
    //   TValue=BlogAuthor → Collection<int, BlogAuthor>
    //   bare "Collection" hint inherits inferred generic args
    // foreach ($authors as $author) → BlogAuthor
    let items = complete_at(&backend, &uri, src, 15, 25).await;
    let names = method_names(&items);
    assert!(
        names.contains(&"posts"),
        "Expected 'posts' from BlogAuthor (demo in separate class, bare Collection hint), got: {:?}",
        names,
    );
    assert!(
        names.contains(&"getName"),
        "Expected 'getName' from BlogAuthor (demo in separate class, bare Collection hint), got: {:?}",
        names,
    );
}

// ─── Namespace breaks closure param inference ───────────────────────────────

/// Closure param inference works in top-level code without a namespace.
/// This is the baseline that passes.
#[tokio::test]
async fn test_closure_param_inference_toplevel_no_namespace() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/closure_toplevel_no_ns.php").unwrap();

    let src = concat!(
        "<?php\n",
        "class QueryBuilder {\n",
        "    /**\n",
        "     * @param (\\Closure(static): mixed)|string|array $column\n",
        "     * @return static\n",
        "     */\n",
        "    public function where($column, $operator = null, $value = null): static { return $this; }\n",
        "    public function whereNotIn(string $col, array $vals): static { return $this; }\n",
        "    public function orWhere(string $col, mixed $val = null): static { return $this; }\n",
        "    public function orderBy(string $col, string $dir = 'asc'): static { return $this; }\n",
        "}\n",
        "$query = QueryBuilder::orderBy('id');\n",
        "$query->where(function ($q): void {\n",
        "    $q->\n",
        "});\n",
    );

    // Line 13: `    $q->` — $q inferred from (\Closure(static): mixed)
    let items = complete_at(&backend, &uri, src, 13, 8).await;
    let names = method_names(&items);
    assert!(
        names.contains(&"whereNotIn"),
        "Expected 'whereNotIn' from QueryBuilder (no namespace), got: {:?}",
        names,
    );
    assert!(
        names.contains(&"orWhere"),
        "Expected 'orWhere' from QueryBuilder (no namespace), got: {:?}",
        names,
    );
}

// ─── Chained static call with closure argument (the actual ProductFilter pattern) ──

/// The real-world Laravel pattern chains the closure directly onto a static
/// call result: `ProductFilter::orderBy('name')->where(function ($query) { $query-> })`.
///
/// This is subtly different from assigning to `$query` first and then calling
/// `$query->where(...)` because the receiver text for `infer_callable_params_from_receiver`
/// is the entire chain `ProductFilter::orderBy('name')` rather than a simple `$query` variable.
#[tokio::test]
async fn test_chained_static_call_closure_param_inference_no_namespace() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/chained_static_closure_no_ns.php").unwrap();

    // Line 0:  <?php
    // Line 1:  class Model {
    // Line 2:      /**
    // Line 3:       * @param (\Closure(static): mixed)|string|array $column
    // Line 4:       * @return static
    // Line 5:       */
    // Line 6:      public function where($column, $operator = null, $value = null): static { return $this; }
    // Line 7:      public function whereNotIn(string $col, array $vals): static { return $this; }
    // Line 8:      public function orWhere(string $col, mixed $val = null): static { return $this; }
    // Line 9:      /** @return static */
    // Line 10:     public static function orderBy(string $col, string $dir = 'asc'): static { return new static(); }
    // Line 11: }
    // Line 12: final class ProductFilter extends Model {}
    // Line 13: $productFilters = ProductFilter::orderBy('name')
    // Line 14:     ->where(function ($query) use ($page): void {
    // Line 15:         $query->
    // Line 16:     });
    let src = concat!(
        "<?php\n",
        "class Model {\n",
        "    /**\n",
        "     * @param (\\Closure(static): mixed)|string|array $column\n",
        "     * @return static\n",
        "     */\n",
        "    public function where($column, $operator = null, $value = null): static { return $this; }\n",
        "    public function whereNotIn(string $col, array $vals): static { return $this; }\n",
        "    public function orWhere(string $col, mixed $val = null): static { return $this; }\n",
        "    /** @return static */\n",
        "    public static function orderBy(string $col, string $dir = 'asc'): static { return new static(); }\n",
        "}\n",
        "final class ProductFilter extends Model {}\n",
        "$page = new \\stdClass();\n",
        "$productFilters = ProductFilter::orderBy('name')\n",
        "    ->where(function ($query) use ($page): void {\n",
        "        $query->\n",
        "    });\n",
    );

    // Line 16: `        $query->` — $query is closure param inferred from
    // `where(@param (\Closure(static): mixed)|…)` on the parent Model class
    let items = complete_at(&backend, &uri, src, 16, 17).await;
    let names = method_names(&items);
    assert!(
        names.contains(&"whereNotIn"),
        "Expected 'whereNotIn' via chained static call closure param (no namespace), got: {:?}",
        names,
    );
    assert!(
        names.contains(&"orWhere"),
        "Expected 'orWhere' via chained static call closure param (no namespace), got: {:?}",
        names,
    );
}

/// Same chained pattern but inside a namespace — the exact pattern from the bug report.
#[tokio::test]
async fn test_chained_static_call_closure_param_inference_with_namespace() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/chained_static_closure_ns.php").unwrap();

    // Line 0:  <?php
    // Line 1:  namespace Acme\Core\Database\Model\Products\Filters;
    // Line 2:  class Model {
    // ...
    // Line 12: }
    // Line 13: final class ProductFilter extends Model {}
    // Line 14: $page = new \stdClass();
    // Line 15: $productFilters = ProductFilter::orderBy('name')
    // Line 16:     ->where(function ($query) use ($page): void {
    // Line 17:         $query->
    // Line 18:     });
    let src = concat!(
        "<?php\n",
        "namespace Acme\\Core\\Database\\Model\\Products\\Filters;\n",
        "class Model {\n",
        "    /**\n",
        "     * @param (\\Closure(static): mixed)|string|array $column\n",
        "     * @return static\n",
        "     */\n",
        "    public function where($column, $operator = null, $value = null): static { return $this; }\n",
        "    public function whereNotIn(string $col, array $vals): static { return $this; }\n",
        "    public function orWhere(string $col, mixed $val = null): static { return $this; }\n",
        "    /** @return static */\n",
        "    public static function orderBy(string $col, string $dir = 'asc'): static { return new static(); }\n",
        "}\n",
        "final class ProductFilter extends Model {}\n",
        "$page = new \\stdClass();\n",
        "$productFilters = ProductFilter::orderBy('name')\n",
        "    ->where(function ($query) use ($page): void {\n",
        "        $query->\n",
        "    });\n",
    );

    // Line 17: `        $query->` — $query is closure param inferred from
    // `where(@param (\Closure(static): mixed)|…)` on the parent Model class
    let items = complete_at(&backend, &uri, src, 17, 17).await;
    let names = method_names(&items);
    assert!(
        names.contains(&"whereNotIn"),
        "Expected 'whereNotIn' via chained static call closure param (with namespace), got: {:?}",
        names,
    );
    assert!(
        names.contains(&"orWhere"),
        "Expected 'orWhere' via chained static call closure param (with namespace), got: {:?}",
        names,
    );
}

/// The chain split across two statements: first assign, then call ->where().
/// This is the simpler variant that the user confirmed works.
#[tokio::test]
async fn test_split_chain_closure_param_inference_with_namespace() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/split_chain_closure_ns.php").unwrap();

    let src = concat!(
        "<?php\n",
        "namespace Acme\\Core\\Database\\Model;\n",
        "class Model {\n",
        "    /**\n",
        "     * @param (\\Closure(static): mixed)|string|array $column\n",
        "     * @return static\n",
        "     */\n",
        "    public function where($column, $operator = null, $value = null): static { return $this; }\n",
        "    public function whereNotIn(string $col, array $vals): static { return $this; }\n",
        "    public function orWhere(string $col, mixed $val = null): static { return $this; }\n",
        "    /** @return static */\n",
        "    public static function orderBy(string $col, string $dir = 'asc'): static { return new static(); }\n",
        "}\n",
        "final class EmailGenerator extends Model {}\n",
        "$query = EmailGenerator::orderBy('id');\n",
        "$query->where(function ($q): void {\n",
        "    $q->\n",
        "});\n",
    );

    // Line 16: `    $q->` — $q inferred from (\Closure(static): mixed)
    let items = complete_at(&backend, &uri, src, 16, 8).await;
    let names = method_names(&items);
    assert!(
        names.contains(&"whereNotIn"),
        "Expected 'whereNotIn' via split chain (namespace), got: {:?}",
        names,
    );
    assert!(
        names.contains(&"orWhere"),
        "Expected 'orWhere' via split chain (namespace), got: {:?}",
        names,
    );
}

/// Same code as above but wrapped in a `namespace` declaration.
/// This reproduces the real-world Laravel bug where the namespace causes
/// closure parameter inference to fail.
#[tokio::test]
async fn test_closure_param_inference_toplevel_with_namespace() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/closure_toplevel_with_ns.php").unwrap();

    let src = concat!(
        "<?php\n",
        "namespace App\\Models;\n",
        "class QueryBuilder {\n",
        "    /**\n",
        "     * @param (\\Closure(static): mixed)|string|array $column\n",
        "     * @return static\n",
        "     */\n",
        "    public function where($column, $operator = null, $value = null): static { return $this; }\n",
        "    public function whereNotIn(string $col, array $vals): static { return $this; }\n",
        "    public function orWhere(string $col, mixed $val = null): static { return $this; }\n",
        "    public function orderBy(string $col, string $dir = 'asc'): static { return $this; }\n",
        "}\n",
        "$query = QueryBuilder::orderBy('id');\n",
        "$query->where(function ($q): void {\n",
        "    $q->\n",
        "});\n",
    );

    // Line 14: `    $q->` — $q inferred from (\Closure(static): mixed)
    let items = complete_at(&backend, &uri, src, 14, 8).await;
    let names = method_names(&items);
    assert!(
        names.contains(&"whereNotIn"),
        "Expected 'whereNotIn' from QueryBuilder (with namespace), got: {:?}",
        names,
    );
    assert!(
        names.contains(&"orWhere"),
        "Expected 'orWhere' from QueryBuilder (with namespace), got: {:?}",
        names,
    );
}

/// Closure param inference inside a class method with a namespace.
/// The method-body path goes through `resolve_variable_in_members`, which
/// is a different code path from top-level code.
#[tokio::test]
async fn test_closure_param_inference_in_method_with_namespace() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/closure_method_with_ns.php").unwrap();

    let src = concat!(
        "<?php\n",
        "namespace App\\Models;\n",
        "class QueryBuilder {\n",
        "    /**\n",
        "     * @param (\\Closure(static): mixed)|string|array $column\n",
        "     * @return static\n",
        "     */\n",
        "    public function where($column, $operator = null, $value = null): static { return $this; }\n",
        "    public function whereNotIn(string $col, array $vals): static { return $this; }\n",
        "    public function orWhere(string $col, mixed $val = null): static { return $this; }\n",
        "    public function orderBy(string $col, string $dir = 'asc'): static { return $this; }\n",
        "}\n",
        "class Service {\n",
        "    public function run(): void {\n",
        "        $query = QueryBuilder::orderBy('id');\n",
        "        $query->where(function ($q): void {\n",
        "            $q->\n",
        "        });\n",
        "    }\n",
        "}\n",
    );

    // Line 16: `            $q->` — $q inferred from (\Closure(static): mixed)
    let items = complete_at(&backend, &uri, src, 16, 17).await;
    let names = method_names(&items);
    assert!(
        names.contains(&"whereNotIn"),
        "Expected 'whereNotIn' from QueryBuilder (method + namespace), got: {:?}",
        names,
    );
    assert!(
        names.contains(&"orWhere"),
        "Expected 'orWhere' from QueryBuilder (method + namespace), got: {:?}",
        names,
    );
}

/// Exact reproduction of the real-world Laravel pattern:
///   - A model class extends a parent that owns `orderBy` and `where`
///   - Top-level code in a namespace calls `Model::orderBy()->where(fn($q) => ...)`
///   - The closure param `$q` should resolve via `(\Closure(static): mixed)`
///
/// The user reported this works without the `namespace` line.  The child
/// class does NOT define `where`/`orderBy` itself — they come from the parent.
#[tokio::test]
async fn test_closure_param_inference_child_class_static_chain_with_namespace() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/closure_child_static_ns.php").unwrap();

    // Line 0:  <?php
    // Line 1:  namespace Acme\Core\Database\Model;
    // Line 2:  class Model {
    // Line 3:      /**
    // Line 4:       * @param (\Closure(static): mixed)|string|array $column
    // Line 5:       * @return static
    // Line 6:       */
    // Line 7:      public function where($column, $operator = null, $value = null): static { return $this; }
    // Line 8:      public function whereNotIn(string $col, array $vals): static { return $this; }
    // Line 9:      public function orWhere(string $col, mixed $val = null): static { return $this; }
    // Line 10:     /** @return static */
    // Line 11:     public static function orderBy(string $col, string $dir = 'asc'): static { return new static(); }
    // Line 12: }
    // Line 13: final class EmailGenerator extends Model {
    // Line 14: }
    // Line 15: $query = EmailGenerator::orderBy('id');
    // Line 16: $query->where(function ($q): void {
    // Line 17:     $q->
    // Line 18: });
    let src = concat!(
        "<?php\n",
        "namespace Acme\\Core\\Database\\Model;\n",
        "class Model {\n",
        "    /**\n",
        "     * @param (\\Closure(static): mixed)|string|array $column\n",
        "     * @return static\n",
        "     */\n",
        "    public function where($column, $operator = null, $value = null): static { return $this; }\n",
        "    public function whereNotIn(string $col, array $vals): static { return $this; }\n",
        "    public function orWhere(string $col, mixed $val = null): static { return $this; }\n",
        "    /** @return static */\n",
        "    public static function orderBy(string $col, string $dir = 'asc'): static { return new static(); }\n",
        "}\n",
        "final class EmailGenerator extends Model {\n",
        "}\n",
        "$query = EmailGenerator::orderBy('id');\n",
        "$query->where(function ($q): void {\n",
        "    $q->\n",
        "});\n",
    );

    // Line 17: `    $q->` — $q is closure param inferred from
    // `where(@param (\Closure(static): mixed)|…)` on the parent Model class
    let items = complete_at(&backend, &uri, src, 17, 8).await;
    let names = method_names(&items);
    assert!(
        names.contains(&"whereNotIn"),
        "Expected 'whereNotIn' from Model via child static chain + namespace, got: {:?}",
        names,
    );
    assert!(
        names.contains(&"orWhere"),
        "Expected 'orWhere' from Model via child static chain + namespace, got: {:?}",
        names,
    );
}

/// Same as above but WITHOUT the namespace — confirms this path works
/// and isolates the namespace as the cause.
#[tokio::test]
async fn test_closure_param_inference_child_class_static_chain_no_namespace() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/closure_child_static_no_ns.php").unwrap();

    let src = concat!(
        "<?php\n",
        "class Model {\n",
        "    /**\n",
        "     * @param (\\Closure(static): mixed)|string|array $column\n",
        "     * @return static\n",
        "     */\n",
        "    public function where($column, $operator = null, $value = null): static { return $this; }\n",
        "    public function whereNotIn(string $col, array $vals): static { return $this; }\n",
        "    public function orWhere(string $col, mixed $val = null): static { return $this; }\n",
        "    /** @return static */\n",
        "    public static function orderBy(string $col, string $dir = 'asc'): static { return new static(); }\n",
        "}\n",
        "final class EmailGenerator extends Model {\n",
        "}\n",
        "$query = EmailGenerator::orderBy('id');\n",
        "$query->where(function ($q): void {\n",
        "    $q->\n",
        "});\n",
    );

    // Line 16: `    $q->` — same test without namespace
    let items = complete_at(&backend, &uri, src, 16, 8).await;
    let names = method_names(&items);
    assert!(
        names.contains(&"whereNotIn"),
        "Expected 'whereNotIn' from Model via child static chain (no namespace), got: {:?}",
        names,
    );
    assert!(
        names.contains(&"orWhere"),
        "Expected 'orWhere' from Model via child static chain (no namespace), got: {:?}",
        names,
    );
}

// ─── Closure with `use ($var)` — does the use-clause break inference? ───────

/// The real-world Laravel pattern uses `function ($query) use ($page): void`.
/// The `use ($page)` clause might cause the parser to produce a different AST
/// node structure that the closure-param inference code doesn't handle.
#[tokio::test]
async fn test_closure_param_inference_with_use_clause() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/closure_use_clause.php").unwrap();

    // Line 0:  <?php
    // Line 1:  class QueryBuilder {
    // Line 2:      /**
    // Line 3:       * @param (\Closure(static): mixed)|string|array $column
    // Line 4:       * @return static
    // Line 5:       */
    // Line 6:      public function where($column, $operator = null, $value = null): static { return $this; }
    // Line 7:      public function whereNotIn(string $col, array $vals): static { return $this; }
    // Line 8:      public function orWhere(string $col, mixed $val = null): static { return $this; }
    // Line 9:      public function orderBy(string $col, string $dir = 'asc'): static { return $this; }
    // Line 10: }
    // Line 11: $page = new \stdClass();
    // Line 12: $query = QueryBuilder::orderBy('id');
    // Line 13: $query->where(function ($q) use ($page): void {
    // Line 14:     $q->
    // Line 15: });
    let src = concat!(
        "<?php\n",
        "class QueryBuilder {\n",
        "    /**\n",
        "     * @param (\\Closure(static): mixed)|string|array $column\n",
        "     * @return static\n",
        "     */\n",
        "    public function where($column, $operator = null, $value = null): static { return $this; }\n",
        "    public function whereNotIn(string $col, array $vals): static { return $this; }\n",
        "    public function orWhere(string $col, mixed $val = null): static { return $this; }\n",
        "    public function orderBy(string $col, string $dir = 'asc'): static { return $this; }\n",
        "}\n",
        "$page = new \\stdClass();\n",
        "$query = QueryBuilder::orderBy('id');\n",
        "$query->where(function ($q) use ($page): void {\n",
        "    $q->\n",
        "});\n",
    );

    let items = complete_at(&backend, &uri, src, 14, 8).await;
    let names = method_names(&items);
    assert!(
        names.contains(&"whereNotIn"),
        "Expected 'whereNotIn' from QueryBuilder via closure with use() clause, got: {:?}",
        names,
    );
    assert!(
        names.contains(&"orWhere"),
        "Expected 'orWhere' from QueryBuilder via closure with use() clause, got: {:?}",
        names,
    );
}

/// Same as above but with a namespace wrapping everything.
#[tokio::test]
async fn test_closure_param_inference_with_use_clause_and_namespace() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/closure_use_clause_ns.php").unwrap();

    // Line 0:  <?php
    // Line 1:  namespace App\Models;
    // Line 2:  class QueryBuilder {
    // ...
    // Line 11: }
    // Line 12: $page = new \stdClass();
    // Line 13: $query = QueryBuilder::orderBy('id');
    // Line 14: $query->where(function ($q) use ($page): void {
    // Line 15:     $q->
    // Line 16: });
    let src = concat!(
        "<?php\n",
        "namespace App\\Models;\n",
        "class QueryBuilder {\n",
        "    /**\n",
        "     * @param (\\Closure(static): mixed)|string|array $column\n",
        "     * @return static\n",
        "     */\n",
        "    public function where($column, $operator = null, $value = null): static { return $this; }\n",
        "    public function whereNotIn(string $col, array $vals): static { return $this; }\n",
        "    public function orWhere(string $col, mixed $val = null): static { return $this; }\n",
        "    public function orderBy(string $col, string $dir = 'asc'): static { return $this; }\n",
        "}\n",
        "$page = new \\stdClass();\n",
        "$query = QueryBuilder::orderBy('id');\n",
        "$query->where(function ($q) use ($page): void {\n",
        "    $q->\n",
        "});\n",
    );

    let items = complete_at(&backend, &uri, src, 15, 8).await;
    let names = method_names(&items);
    assert!(
        names.contains(&"whereNotIn"),
        "Expected 'whereNotIn' from QueryBuilder via closure with use() + namespace, got: {:?}",
        names,
    );
    assert!(
        names.contains(&"orWhere"),
        "Expected 'orWhere' from QueryBuilder via closure with use() + namespace, got: {:?}",
        names,
    );
}

// ─── Cross-file PSR-4: closure param inference with namespace ───────────────

/// Cross-file reproduction of the real-world Laravel pattern.
///
/// The `where` method lives on a parent `Model` class in a separate file.
/// `EmailGenerator extends Model` in another file.  Top-level code in a
/// namespace calls `EmailGenerator::orderBy('id')->where(function ($q) { $q-> })`.
///
/// The `$q` closure param should be inferred from
/// `@param (\Closure(static): mixed)|string|array $column` on the parent.
#[tokio::test]
async fn test_cross_file_closure_param_inference_with_namespace() {
    let composer = r#"{"autoload": {"psr-4": {"App\\": "src/"}}}"#;

    let model_file = concat!(
        "<?php\n",
        "namespace App\\Database;\n",
        "class Model {\n",
        "    /**\n",
        "     * @param (\\Closure(static): mixed)|string|array $column\n",
        "     * @return static\n",
        "     */\n",
        "    public function where($column, $operator = null, $value = null): static { return $this; }\n",
        "    public function whereNotIn(string $col, array $vals): static { return $this; }\n",
        "    public function orWhere(string $col, mixed $val = null): static { return $this; }\n",
        "    /** @return static */\n",
        "    public static function orderBy(string $col, string $dir = 'asc'): static { return new static(); }\n",
        "}\n",
    );

    let generator_file = concat!(
        "<?php\n",
        "namespace App\\Models;\n",
        "use App\\Database\\Model;\n",
        "final class EmailGenerator extends Model {\n",
        "}\n",
    );

    // Line 0: <?php
    // Line 1: namespace App\Script;
    // Line 2: use App\Models\EmailGenerator;
    // Line 3: $query = EmailGenerator::orderBy('id');
    // Line 4: $query->where(function ($q): void {
    // Line 5:     $q->
    // Line 6: });
    let script_file = concat!(
        "<?php\n",
        "namespace App\\Script;\n",
        "use App\\Models\\EmailGenerator;\n",
        "$query = EmailGenerator::orderBy('id');\n",
        "$query->where(function ($q): void {\n",
        "    $q->\n",
        "});\n",
    );

    let (backend, dir) = create_psr4_workspace(
        composer,
        &[
            ("src/Database/Model.php", model_file),
            ("src/Models/EmailGenerator.php", generator_file),
            ("src/Script/run.php", script_file),
        ],
    );

    let uri = Url::from_file_path(dir.path().join("src/Script/run.php")).unwrap();

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: script_file.to_string(),
        },
    };
    backend.did_open(open_params).await;

    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 5,
                character: 8,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let items = match backend.completion(completion_params).await.unwrap() {
        Some(CompletionResponse::Array(items)) => items,
        Some(CompletionResponse::List(list)) => list.items,
        None => vec![],
    };

    let names = method_names(&items);
    assert!(
        names.contains(&"whereNotIn"),
        "Expected 'whereNotIn' from Model via cross-file closure param inference, got: {:?}",
        names,
    );
    assert!(
        names.contains(&"orWhere"),
        "Expected 'orWhere' from Model via cross-file closure param inference, got: {:?}",
        names,
    );
}

/// Same cross-file layout but WITHOUT the namespace in the script file.
/// Confirms that removing the namespace makes it work (matching the user's report).
#[tokio::test]
async fn test_cross_file_closure_param_inference_without_namespace() {
    let composer = r#"{"autoload": {"psr-4": {"App\\": "src/"}}}"#;

    let model_file = concat!(
        "<?php\n",
        "namespace App\\Database;\n",
        "class Model {\n",
        "    /**\n",
        "     * @param (\\Closure(static): mixed)|string|array $column\n",
        "     * @return static\n",
        "     */\n",
        "    public function where($column, $operator = null, $value = null): static { return $this; }\n",
        "    public function whereNotIn(string $col, array $vals): static { return $this; }\n",
        "    public function orWhere(string $col, mixed $val = null): static { return $this; }\n",
        "    /** @return static */\n",
        "    public static function orderBy(string $col, string $dir = 'asc'): static { return new static(); }\n",
        "}\n",
    );

    let generator_file = concat!(
        "<?php\n",
        "namespace App\\Models;\n",
        "use App\\Database\\Model;\n",
        "final class EmailGenerator extends Model {\n",
        "}\n",
    );

    // No namespace — uses FQN in the `use` import
    // Line 0: <?php
    // Line 1: use App\Models\EmailGenerator;
    // Line 2: $query = EmailGenerator::orderBy('id');
    // Line 3: $query->where(function ($q): void {
    // Line 4:     $q->
    // Line 5: });
    let script_file = concat!(
        "<?php\n",
        "use App\\Models\\EmailGenerator;\n",
        "$query = EmailGenerator::orderBy('id');\n",
        "$query->where(function ($q): void {\n",
        "    $q->\n",
        "});\n",
    );

    let (backend, dir) = create_psr4_workspace(
        composer,
        &[
            ("src/Database/Model.php", model_file),
            ("src/Models/EmailGenerator.php", generator_file),
            ("src/Script/run.php", script_file),
        ],
    );

    let uri = Url::from_file_path(dir.path().join("src/Script/run.php")).unwrap();

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: script_file.to_string(),
        },
    };
    backend.did_open(open_params).await;

    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 4,
                character: 8,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let items = match backend.completion(completion_params).await.unwrap() {
        Some(CompletionResponse::Array(items)) => items,
        Some(CompletionResponse::List(list)) => list.items,
        None => vec![],
    };

    let names = method_names(&items);
    assert!(
        names.contains(&"whereNotIn"),
        "Expected 'whereNotIn' from Model via cross-file (no namespace), got: {:?}",
        names,
    );
    assert!(
        names.contains(&"orWhere"),
        "Expected 'orWhere' from Model via cross-file (no namespace), got: {:?}",
        names,
    );
}

// ─── Cross-file with real Laravel-like Eloquent Builder vendor structure ─────

/// Simulates the real-world Laravel stack where `where()` lives on
/// `Illuminate\Database\Eloquent\Builder` (loaded from vendor), the model
/// extends `Illuminate\Database\Eloquent\Model`, and the closure param
/// should be inferred through the builder-as-static forwarding mechanism.
///
/// This is the exact scenario from the bug report:
///
/// ```php
/// namespace App\Http\Controllers;
/// use Acme\Core\Database\Model\EmailGenerator;
/// $query = EmailGenerator::orderBy('id');
/// $query->where(function ($q): void {
///     $q->where('domain', 'like', '%b%');  // $q should resolve
/// });
/// ```
///
/// The key difference from simpler tests is that `where()` and `orderBy()`
/// are NOT defined directly on the user's model class. They come from the
/// Eloquent Builder which is loaded from vendor and forwarded onto the model
/// as static virtual methods by the LaravelModelProvider.
#[tokio::test]
async fn test_cross_file_laravel_eloquent_builder_closure_param_inference() {
    let composer = r#"{
        "autoload": {
            "psr-4": {
                "App\\": "src/",
                "Acme\\Core\\Database\\Model\\": "src/Models/",
                "Illuminate\\Database\\Eloquent\\": "vendor/laravel/framework/src/Illuminate/Database/Eloquent/",
                "Illuminate\\Database\\Query\\": "vendor/laravel/framework/src/Illuminate/Database/Query/"
            }
        }
    }"#;

    // Minimal Eloquent Builder with the exact @param from real Laravel
    let eloquent_builder_file = concat!(
        "<?php\n",
        "namespace Illuminate\\Database\\Eloquent;\n",
        "\n",
        "/**\n",
        " * @template TModel of Model\n",
        " * @mixin \\Illuminate\\Database\\Query\\Builder\n",
        " */\n",
        "class Builder {\n",
        "    /**\n",
        "     * @param  (\\Closure(static): mixed)|string|array  $column\n",
        "     * @return $this\n",
        "     */\n",
        "    public function where($column, $operator = null, $value = null, $boolean = 'and') { return $this; }\n",
        "\n",
        "    /** @return $this */\n",
        "    public function orWhere($column, $operator = null, $value = null) { return $this; }\n",
        "\n",
        "    /** @return $this */\n",
        "    public function whereNotIn(string $column, array $values) { return $this; }\n",
        "\n",
        "    /** @return $this */\n",
        "    public function orderBy(string $column, string $direction = 'asc') { return $this; }\n",
        "}\n",
    );

    // Minimal Query Builder (the @mixin target)
    let query_builder_file = concat!(
        "<?php\n",
        "namespace Illuminate\\Database\\Query;\n",
        "\n",
        "class Builder {\n",
        "    /** @return $this */\n",
        "    public function groupBy(...$groups) { return $this; }\n",
        "}\n",
    );

    // Minimal Eloquent Model
    let model_file = concat!(
        "<?php\n",
        "namespace Illuminate\\Database\\Eloquent;\n",
        "\n",
        "abstract class Model {\n",
        "}\n",
    );

    // User's model class
    let email_generator_file = concat!(
        "<?php\n",
        "namespace Acme\\Core\\Database\\Model;\n",
        "\n",
        "use Illuminate\\Database\\Eloquent\\Model;\n",
        "\n",
        "final class EmailGenerator extends Model {\n",
        "}\n",
    );

    // The script file — exact pattern from the bug report
    // Line 0: <?php
    // Line 1: namespace App\Http\Controllers;
    // Line 2: use Acme\Core\Database\Model\EmailGenerator;
    // Line 3: $query = EmailGenerator::orderBy('id');
    // Line 4: $query->where(function ($q): void {
    // Line 5:     $q->
    // Line 6: });
    let script_file = concat!(
        "<?php\n",
        "namespace App\\Http\\Controllers;\n",
        "use Acme\\Core\\Database\\Model\\EmailGenerator;\n",
        "$query = EmailGenerator::orderBy('id');\n",
        "$query->where(function ($q): void {\n",
        "    $q->\n",
        "});\n",
    );

    let (backend, dir) = create_psr4_workspace(
        composer,
        &[
            (
                "vendor/laravel/framework/src/Illuminate/Database/Eloquent/Builder.php",
                eloquent_builder_file,
            ),
            (
                "vendor/laravel/framework/src/Illuminate/Database/Query/Builder.php",
                query_builder_file,
            ),
            (
                "vendor/laravel/framework/src/Illuminate/Database/Eloquent/Model.php",
                model_file,
            ),
            ("src/Models/EmailGenerator.php", email_generator_file),
            ("src/Http/Controllers/script.php", script_file),
        ],
    );

    let uri = Url::from_file_path(dir.path().join("src/Http/Controllers/script.php")).unwrap();

    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: script_file.to_string(),
        },
    };
    backend.did_open(open_params).await;

    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: 5,
                character: 8,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let items = match backend.completion(completion_params).await.unwrap() {
        Some(CompletionResponse::Array(items)) => items,
        Some(CompletionResponse::List(list)) => list.items,
        None => vec![],
    };

    let names = method_names(&items);
    assert!(
        names.contains(&"where"),
        "Expected 'where' on $q via Eloquent Builder closure param inference, got: {:?}",
        names,
    );
    assert!(
        names.contains(&"orWhere"),
        "Expected 'orWhere' on $q via Eloquent Builder closure param inference, got: {:?}",
        names,
    );
}

// ─── Inferred subclass type wins over explicit parent type hint ─────────────

/// When a closure parameter has an explicit type hint that is a parent class
/// (e.g. `Model`) but the inferred type from the callable signature is a
/// subclass (e.g. `BrandTranslation extends Model`), the inferred subclass
/// type should win because it is more specific.
///
/// Real-world example:
/// ```php
/// $this->getQuery(markets: $markets)
///     ->each(function (Model $brandTranslation) {
///         $brandTranslation->  // should resolve as BrandTranslation, not Model
///     });
/// ```
#[tokio::test]
async fn test_inferred_subclass_wins_over_explicit_parent_type_hint() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/closure_inferred_subclass.php").unwrap();

    let src = concat!(
        "<?php\n",
        "class Model {\n",
        "    public function save(): bool { return true; }\n",
        "}\n",
        "class BrandTranslation extends Model {\n",
        "    public function getLangCode(): string { return ''; }\n",
        "    public function getBrandName(): string { return ''; }\n",
        "}\n",
        "/**\n",
        " * @template TKey\n",
        " * @template TValue\n",
        " */\n",
        "class Collection {\n",
        "    /**\n",
        "     * @param callable(TValue): mixed $callback\n",
        "     * @return static\n",
        "     */\n",
        "    public function each(callable $callback): static {}\n",
        "}\n",
        "class BrandService {\n",
        "    /** @return Collection<int, BrandTranslation> */\n",
        "    public function getTranslations(): Collection {}\n",
        "    public function run(): void {\n",
        "        $translations = $this->getTranslations();\n",
        "        $translations->each(function (Model $brandTranslation) {\n",
        "            $brandTranslation->\n",
        "        });\n",
        "    }\n",
        "}\n",
    );

    // Line 25: `            $brandTranslation->`  cursor after `->`
    let items = complete_at(&backend, &uri, src, 25, 31).await;
    let names = method_names(&items);
    // BrandTranslation-specific methods should be present
    assert!(
        names.contains(&"getLangCode"),
        "Inferred BrandTranslation should win over explicit Model; expected getLangCode in {:?}",
        names,
    );
    assert!(
        names.contains(&"getBrandName"),
        "Inferred BrandTranslation should win over explicit Model; expected getBrandName in {:?}",
        names,
    );
    // Parent Model methods should also be present (inherited)
    assert!(
        names.contains(&"save"),
        "Inherited Model methods should still be present; expected save in {:?}",
        names,
    );
}

/// When the explicit type hint is already the most specific type (i.e. the
/// inferred type is a parent, not a subclass), the explicit type should
/// still win.  This is the inverse of the above test and ensures we don't
/// regress `test_explicit_type_hint_takes_precedence`.
#[tokio::test]
async fn test_explicit_subclass_still_wins_over_inferred_parent() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/closure_explicit_subclass.php").unwrap();

    let src = concat!(
        "<?php\n",
        "class Animal {\n",
        "    public function speak(): string { return ''; }\n",
        "}\n",
        "class Cat extends Animal {\n",
        "    public function purr(): void {}\n",
        "}\n",
        "/**\n",
        " * @template TKey\n",
        " * @template TValue\n",
        " */\n",
        "class Collection {\n",
        "    /**\n",
        "     * @param callable(TValue): mixed $callback\n",
        "     * @return static\n",
        "     */\n",
        "    public function each(callable $callback): static {}\n",
        "}\n",
        "class Shelter {\n",
        "    /** @return Collection<int, Animal> */\n",
        "    public function getAnimals(): Collection {}\n",
        "    public function run(): void {\n",
        "        $animals = $this->getAnimals();\n",
        "        $animals->each(function (Cat $c) {\n",
        "            $c->\n",
        "        });\n",
        "    }\n",
        "}\n",
    );

    // Line 24: `            $c->`  cursor after `->`
    let items = complete_at(&backend, &uri, src, 24, 17).await;
    let names = method_names(&items);
    // Cat-specific method should be present (explicit hint is more specific)
    assert!(
        names.contains(&"purr"),
        "Explicit Cat type should win over inferred Animal; expected purr in {:?}",
        names,
    );
}
// ─── Closure with explicit type hint nested inside arrow function body ───────

/// When a closure with an explicit type hint is nested inside an arrow
/// function's body expression (e.g. `fn($r) => $r->method(function (Foo $q) { $q-> })`),
/// the closure parameter should still resolve from its type hint.
#[tokio::test]
async fn test_closure_with_type_hint_nested_inside_arrow_fn() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/closure_nested_in_arrow.php").unwrap();

    let src = concat!(
        "<?php\n",
        "class Builder {\n",
        "    public function where(string $col, mixed $val = null): static { return $this; }\n",
        "    public function orderBy(string $col): static { return $this; }\n",
        "    /**\n",
        "     * @param string $relation\n",
        "     * @param (\\Closure(static): mixed)|null $callback\n",
        "     * @return static\n",
        "     */\n",
        "    public function whereHas(string $relation, ?callable $callback = null): static { return $this; }\n",
        "}\n",
        "class Relation {\n",
        "    public function whereHas(string $relation, ?callable $callback = null): static { return $this; }\n",
        "}\n",
        "class Service {\n",
        "    public function run(): void {\n",
        "        $items = [fn(Relation $r): Relation => $r->whereHas('locales', function (Builder $q): void {\n",
        "            $q->\n",
        "        })];\n",
        "    }\n",
        "}\n",
    );

    // Line 17: `            $q->`  cursor after `->`
    let items = complete_at(&backend, &uri, src, 17, 16).await;
    let names = method_names(&items);
    assert!(
        names.contains(&"where"),
        "Expected where() from explicitly typed Builder $q nested in arrow fn, got: {:?}",
        names,
    );
    assert!(
        names.contains(&"orderBy"),
        "Expected orderBy() from explicitly typed Builder $q nested in arrow fn, got: {:?}",
        names,
    );
}

// ─── Arrow fn inside switch case body ───────────────────────────────────────

/// When a closure or arrow function with an explicit type hint appears inside
/// a `switch` case body, `try_resolve_in_closure_stmt` must recurse into the
/// switch cases.  Previously `Switch` was not handled, so the parameter's
/// explicit type hint was lost.
#[tokio::test]
async fn test_arrow_fn_with_explicit_hint_inside_switch_case() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/closure_in_switch.php").unwrap();

    let src = concat!(
        "<?php\n",
        "class DeviationMessage {\n",
        "    public string $type = '';\n",
        "    public int $productId = 0;\n",
        "    public int $lineNumber = 0;\n",
        "}\n",
        "class Checker {\n",
        "    /** @var array<int, DeviationMessage> */\n",
        "    private array $items = [];\n",
        "    public function check(string $kind): bool {\n",
        "        switch ($kind) {\n",
        "            case 'a':\n",
        "                return array_any($this->items, fn(DeviationMessage $item) => $item->type === 'x' && $item->productId === 1);\n",
        "            case 'b':\n",
        "                return array_any($this->items, fn(DeviationMessage $item) => $item->type === 'y' && $item->lineNumber === 2);\n",
        "            default:\n",
        "                return false;\n",
        "        }\n",
        "    }\n",
        "}\n",
    );

    // Line 12: case 'a' body — `fn(DeviationMessage $item) => $item->type`
    //   `                return array_any($this->items, fn(DeviationMessage $item) => $item->type === 'x' && $item->productId === 1);`
    //                                                                                       ^ col 87 is after `$item->`
    let items_a = complete_at(&backend, &uri, src, 12, 87).await;
    let names_a = property_names(&items_a);
    assert!(
        names_a.contains(&"type"),
        "Expected 'type' from DeviationMessage in switch case 'a', got: {:?}",
        names_a,
    );
    assert!(
        names_a.contains(&"productId"),
        "Expected 'productId' from DeviationMessage in switch case 'a', got: {:?}",
        names_a,
    );

    // Line 14: case 'b' body — same pattern, second case
    let items_b = complete_at(&backend, &uri, src, 14, 87).await;
    let names_b = property_names(&items_b);
    assert!(
        names_b.contains(&"type"),
        "Expected 'type' from DeviationMessage in switch case 'b', got: {:?}",
        names_b,
    );
    assert!(
        names_b.contains(&"lineNumber"),
        "Expected 'lineNumber' from DeviationMessage in switch case 'b', got: {:?}",
        names_b,
    );
}

// ─── Arrow fn inside if condition expression ────────────────────────────────

/// When a closure or arrow function appears inside an `if` condition
/// (e.g. `if (array_any(..., fn(Foo $x) => $x->bar)) { ... }`),
/// `try_resolve_in_closure_stmt` must recurse into the condition
/// expression, not just the body.
#[tokio::test]
async fn test_arrow_fn_with_explicit_hint_inside_if_condition() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/closure_in_if_cond.php").unwrap();

    let src = concat!(
        "<?php\n",
        "class DeviationMessage {\n",
        "    public string $type = '';\n",
        "    public int $productId = 0;\n",
        "}\n",
        "class Checker {\n",
        "    /** @var array<int, DeviationMessage> */\n",
        "    private array $items = [];\n",
        "    public function check(): void {\n",
        "        if (array_any($this->items, fn(DeviationMessage $item) => $item->type === 'x' && $item->productId === 1)) {\n",
        "            return;\n",
        "        }\n",
        "    }\n",
        "}\n",
    );

    // Line 9: `        if (array_any($this->items, fn(DeviationMessage $item) => $item->type ...`
    //   `$item->` starts at col 76
    let items = complete_at(&backend, &uri, src, 9, 77).await;
    let names = property_names(&items);
    assert!(
        names.contains(&"type"),
        "Expected 'type' from DeviationMessage in if condition, got: {:?}",
        names,
    );
    assert!(
        names.contains(&"productId"),
        "Expected 'productId' from DeviationMessage in if condition, got: {:?}",
        names,
    );
}

// ─── Arrow fn in if condition inside switch case (real-world nesting) ────────

/// The exact nesting from `PurchaseFileDeviationMessageCollection`: a closure
/// inside an `if` condition inside a `switch` case body.  Both `switch` and
/// `if`-condition recursion must work together.
#[tokio::test]
async fn test_arrow_fn_in_if_condition_inside_switch_case() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/closure_if_in_switch.php").unwrap();

    let src = concat!(
        "<?php\n",
        "class DeviationMessage {\n",
        "    public string $type = '';\n",
        "    public int $productId = 0;\n",
        "    public int $lineNumber = 0;\n",
        "}\n",
        "class Checker {\n",
        "    /** @var array<int, DeviationMessage> */\n",
        "    private array $items = [];\n",
        "    public function check(string $kind): void {\n",
        "        switch ($kind) {\n",
        "            case 'a':\n",
        "                if (array_any($this->items, fn(DeviationMessage $item) => $item->type === 'x' && $item->productId === 1)) {\n",
        "                    return;\n",
        "                }\n",
        "                break;\n",
        "            case 'b':\n",
        "                if (array_any($this->items, fn(DeviationMessage $item) => $item->type === 'y' && $item->lineNumber === 2)) {\n",
        "                    return;\n",
        "                }\n",
        "                break;\n",
        "        }\n",
        "    }\n",
        "}\n",
    );

    // Line 12: case 'a' — if condition contains the arrow fn
    //   `                if (array_any($this->items, fn(DeviationMessage $item) => $item->type ...`
    //                                                                                    ^ col 91 after `$item->`
    let items_a = complete_at(&backend, &uri, src, 12, 81).await;
    let names_a = property_names(&items_a);
    assert!(
        names_a.contains(&"type"),
        "Expected 'type' from DeviationMessage in if-in-switch case 'a', got: {:?}",
        names_a,
    );
    assert!(
        names_a.contains(&"productId"),
        "Expected 'productId' from DeviationMessage in if-in-switch case 'a', got: {:?}",
        names_a,
    );

    // Line 17: case 'b' — same nesting
    let items_b = complete_at(&backend, &uri, src, 17, 81).await;
    let names_b = property_names(&items_b);
    assert!(
        names_b.contains(&"type"),
        "Expected 'type' from DeviationMessage in if-in-switch case 'b', got: {:?}",
        names_b,
    );
    assert!(
        names_b.contains(&"lineNumber"),
        "Expected 'lineNumber' from DeviationMessage in if-in-switch case 'b', got: {:?}",
        names_b,
    );
}

/// Simpler case: standalone function with @template and callable param,
/// variable passed directly (no `$this->` involved).
#[tokio::test]
async fn test_arrow_fn_untyped_param_inferred_from_function_template_simple() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///template_callable_simple.php").unwrap();

    let src = concat!(
        "<?php\n",                                                                        // 0
        "/**\n",                                                                          // 1
        " * @template TKey\n",                                                            // 2
        " * @template TValue\n",                                                          // 3
        " * @param array<TKey, TValue> $array\n",                                         // 4
        " * @param callable(TValue, TKey): bool $callback\n",                             // 5
        " * @return bool\n",                                                              // 6
        " */\n",                                                                          // 7
        "function array_any(array $array, callable $callback): bool { return false; }\n", // 8
        "\n",                                                                             // 9
        "class Product {\n",                                                              // 10
        "    public int $amount = 0;\n",                                                  // 11
        "    public string $sku = '';\n",                                                 // 12
        "}\n",                                                                            // 13
        "\n",                                                                             // 14
        "/** @var array<int, Product> $products */\n",                                    // 15
        "$products = [];\n",                                                              // 16
        "array_any($products, fn($item) => $item->amount > 0);\n",                        // 17
    );

    // `$item->` on line 17
    // "array_any($products, fn($item) => $item->amount > 0);"
    //                                          ^ after `$item->`
    let items = complete_at(&backend, &uri, src, 17, 42).await;
    let props = property_names(&items);
    assert!(
        props.contains(&"amount"),
        "Expected 'amount' inferred from function template binding, got props: {:?}",
        props,
    );
    assert!(
        props.contains(&"sku"),
        "Expected 'sku' inferred from function template binding, got props: {:?}",
        props,
    );
}

/// When `array_any($this->items, fn($item) => $item->...)` is used and
/// `$item` has NO explicit type hint, the type should be inferred from
/// the function's `@template` binding.  `array_any` declares
/// `@param array<TKey, TValue> $array` and
/// `@param callable(TValue, TKey): bool $callback`, so the concrete
/// element type flows from the first argument into the closure param.
#[tokio::test]
async fn test_arrow_fn_untyped_param_inferred_from_function_template() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///template_callable_infer.php").unwrap();

    let src = concat!(
        "<?php\n",
        "/**\n",
        " * @template TKey\n",
        " * @template TValue\n",
        " * @param array<TKey, TValue> $array\n",
        " * @param callable(TValue, TKey): bool $callback\n",
        " * @return bool\n",
        " */\n",
        "function array_any(array $array, callable $callback): bool { return false; }\n",
        "\n",
        "class PurchaseFileProduct {\n",
        "    public int $order_amount = 0;\n",
        "    public string $name = '';\n",
        "}\n",
        "\n",
        "/**\n",
        " * @template TKey\n",
        " * @template TValue\n",
        " */\n",
        "class Collection {\n",
        "    /** @var array<TKey, TValue> */\n",
        "    public array $items = [];\n",
        "}\n",
        "\n",
        "/** @extends Collection<int, PurchaseFileProduct> */\n",
        "final class PurchaseFileProductCollection extends Collection {\n",
        "    public function hasIssues(): bool {\n",
        "        return array_any($this->items, fn($item) => $item->order_amount > 0);\n",
        "    }\n",
        "}\n",
    );

    // `$item->` is inside the arrow fn on line 27
    // "        return array_any($this->items, fn($item) => $item->order_amount > 0);"
    //                                                            ^ after `$item->`
    let items = complete_at(&backend, &uri, src, 27, 62).await;
    let props = property_names(&items);
    assert!(
        props.contains(&"order_amount"),
        "Expected 'order_amount' inferred from function template binding, got props: {:?}",
        props,
    );
    assert!(
        props.contains(&"name"),
        "Expected 'name' inferred from function template binding, got props: {:?}",
        props,
    );
}

// ─── Generic type_string preservation through self/static returns ───────────

/// When a method returns `static` on a generic class, the `ResolvedType`
/// `type_string` must carry the reconstructed generic args (e.g.
/// `Builder<Customer>` instead of bare `static`).  Without this, downstream
/// consumers like `build_receiver_template_subs` see no generic args and
/// cannot substitute template parameters in callable params.
///
/// Scenario: `Customer2::where(…)->where(…)->each(fn($items) { $items-> })`
///
/// - `Customer2::where()` explicitly returns `Builder<Customer>`
/// - `Builder::where()` returns `static`
/// - The second `where()` must produce `Builder<Customer>` (not bare `static`)
/// - `each(callable(Collection<int, TModel>))` must substitute `TModel=Customer`
/// - `$items` should resolve to `Collection<int, Customer>` with `first()` etc.
#[tokio::test]
async fn test_static_return_preserves_generic_type_string_in_chain() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/static_generic_chain.php").unwrap();

    let src = concat!(
        "<?php\n",
        "class Customer {\n",
        "    public function isActive(): bool { return true; }\n",
        "    public function getEmail(): string { return ''; }\n",
        "}\n",
        "/**\n",
        " * @template TKey of array-key\n",
        " * @template TValue\n",
        " */\n",
        "class Collection {\n",
        "    /** @return TValue|null */\n",
        "    public function first(): mixed { return null; }\n",
        "    /** @return int */\n",
        "    public function count(): int { return 0; }\n",
        "}\n",
        "/**\n",
        " * @template TModel\n",
        " */\n",
        "class Builder {\n",
        "    /**\n",
        "     * @param callable(Collection<int, TModel>): mixed $callback\n",
        "     * @return bool\n",
        "     */\n",
        "    public function each(callable $callback): bool { return true; }\n",
        "    /** @return static */\n",
        "    public function where(string $col, mixed $val = null): static { return $this; }\n",
        "    /** @return static */\n",
        "    public function orderBy(string $col): static { return $this; }\n",
        "}\n",
        "class CustomerQuery {\n",
        "    /** @return Builder<Customer> */\n",
        "    public static function where(string $col, mixed $val = null): Builder { return new Builder(); }\n",
        "}\n",
        "class Service {\n",
        "    public function run(): void {\n",
        // Line 34: CustomerQuery::where()->orderBy()->each(fn($items) { $items-> })
        // The chain is: CustomerQuery::where() => Builder<Customer>
        //               ->orderBy() => static (should reconstruct to Builder<Customer>)
        //               ->each(fn($items) => ...) => $items is Collection<int, Customer>
        "        CustomerQuery::where('active', true)->orderBy('name')->each(function ($items) {\n",
        "            $items->\n",
        "        });\n",
        "    }\n",
        "}\n",
    );

    // Line 36: `            $items->` — $items is inferred from each()'s
    // callable param after template substitution through the static chain.
    let items = complete_at(&backend, &uri, src, 36, 20).await;
    let names = method_names(&items);
    assert!(
        names.contains(&"first"),
        "Expected 'first' from Collection on closure param after static return chain, got: {:?}",
        names,
    );
    assert!(
        names.contains(&"count"),
        "Expected 'count' from Collection on closure param after static return chain, got: {:?}",
        names,
    );
}

/// Same as above but with two chained `static` returns to verify the
/// generic type string propagates through multiple hops.
#[tokio::test]
async fn test_multiple_static_returns_preserve_generic_type_string() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/multi_static_chain.php").unwrap();

    let src = concat!(
        "<?php\n",
        "class Product {\n",
        "    public string $name = '';\n",
        "    public int $price = 0;\n",
        "}\n",
        "/**\n",
        " * @template TKey of array-key\n",
        " * @template TValue\n",
        " */\n",
        "class Collection {\n",
        "    /** @return TValue|null */\n",
        "    public function first(): mixed { return null; }\n",
        "}\n",
        "/**\n",
        " * @template TModel\n",
        " */\n",
        "class Builder {\n",
        "    /**\n",
        "     * @param callable(Collection<int, TModel>): mixed $callback\n",
        "     * @return bool\n",
        "     */\n",
        "    public function chunk(int $count, callable $callback): bool { return true; }\n",
        "    /** @return static */\n",
        "    public function where(string $col, mixed $val = null): static { return $this; }\n",
        "    /** @return static */\n",
        "    public function orderBy(string $col): static { return $this; }\n",
        "    /** @return static */\n",
        "    public function limit(int $n): static { return $this; }\n",
        "}\n",
        "class ProductQuery {\n",
        "    /** @return Builder<Product> */\n",
        "    public static function query(): Builder { return new Builder(); }\n",
        "}\n",
        "class Service {\n",
        "    public function run(): void {\n",
        // Three chained static returns: query() => Builder<Product>
        // ->where() => static => Builder<Product>
        // ->orderBy() => static => Builder<Product>
        // ->limit() => static => Builder<Product>
        // ->chunk(100, fn($items) => $items->first()->...)
        "        ProductQuery::query()->where('active', 1)->orderBy('name')->limit(50)->chunk(100, function ($items) {\n",
        "            $first = $items->first();\n",
        "            $first->\n",
        "        });\n",
        "    }\n",
        "}\n",
    );

    // Line 37: `            $first->` — $first = $items->first() where
    // $items is Collection<int, Product>, so first() returns Product|null.
    let items = complete_at(&backend, &uri, src, 37, 20).await;
    let props = property_names(&items);
    assert!(
        props.contains(&"name"),
        "Expected 'name' from Product after multiple static returns in chain, got props: {:?}",
        props,
    );
    assert!(
        props.contains(&"price"),
        "Expected 'price' from Product after multiple static returns in chain, got props: {:?}",
        props,
    );
}
