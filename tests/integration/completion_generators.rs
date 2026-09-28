use crate::common::{complete_at, create_psr4_workspace, create_test_backend};
use tower_lsp::lsp_types::*;

// ─── Foreach over Generator with @var annotation ────────────────────────────

/// When a variable is annotated as `Generator<int, User>` and iterated with
/// foreach, the value variable should resolve to `User`.
#[tokio::test]
async fn test_foreach_generator_var_annotation_two_params() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///gen_var_two.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class User {\n",
        "    public string $name;\n",
        "    public function getEmail(): string {}\n",
        "}\n",
        "class Service {\n",
        "    public function process() {\n",
        "        /** @var \\Generator<int, User> $gen */\n",
        "        $gen = $this->getUsers();\n",
        "        foreach ($gen as $user) {\n",
        "            $user->\n",
        "        }\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 10, 19).await;
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(
        labels.iter().any(|l| l.starts_with("name")),
        "Should include 'name' from User via Generator<int, User> foreach. Got: {:?}",
        labels
    );
    assert!(
        labels.iter().any(|l| l.starts_with("getEmail")),
        "Should include 'getEmail' from User via Generator<int, User> foreach. Got: {:?}",
        labels
    );
}

/// When `Generator` has a single type parameter, it should be treated as the
/// value type (the yield type).
#[tokio::test]
async fn test_foreach_generator_var_annotation_single_param() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///gen_var_single.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Product {\n",
        "    public string $title;\n",
        "    public function getPrice(): float {}\n",
        "}\n",
        "class Loader {\n",
        "    public function load() {\n",
        "        /** @var \\Generator<Product> $gen */\n",
        "        $gen = $this->loadProducts();\n",
        "        foreach ($gen as $product) {\n",
        "            $product->\n",
        "        }\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 10, 22).await;
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(
        labels.iter().any(|l| l.starts_with("title")),
        "Should include 'title' from Product via Generator<Product> foreach. Got: {:?}",
        labels
    );
}

/// When `Generator` has all four type parameters, the value (yield) type is
/// still the second parameter.
#[tokio::test]
async fn test_foreach_generator_var_annotation_four_params() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///gen_var_four.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Order {\n",
        "    public int $id;\n",
        "    public function getTotal(): float {}\n",
        "}\n",
        "class Response {\n",
        "    public int $statusCode;\n",
        "}\n",
        "class Pipeline {\n",
        "    public function run() {\n",
        "        /** @var \\Generator<int, Order, mixed, Response> $gen */\n",
        "        $gen = $this->process();\n",
        "        foreach ($gen as $order) {\n",
        "            $order->\n",
        "        }\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 13, 20).await;
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(
        labels.iter().any(|l| l.starts_with("id")),
        "Should include 'id' from Order (2nd param of Generator<int, Order, mixed, Response>). Got: {:?}",
        labels
    );
    assert!(
        labels.iter().any(|l| l.starts_with("getTotal")),
        "Should include 'getTotal' from Order (2nd param). Got: {:?}",
        labels
    );
    // Make sure we're NOT getting Response members (4th param / TReturn).
    assert!(
        !labels.iter().any(|l| l.starts_with("statusCode")),
        "Should NOT include 'statusCode' from Response (4th param / TReturn). Got: {:?}",
        labels
    );
}

// ─── Foreach over Generator from method return type ─────────────────────────

/// When a method's `@return` specifies `Generator<int, User>`, iterating
/// the method call result should resolve the value variable to `User`.
#[tokio::test]
async fn test_foreach_generator_method_return_type() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///gen_method_ret.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Customer {\n",
        "    public string $name;\n",
        "    public function getAddress(): string {}\n",
        "}\n",
        "class CustomerRepository {\n",
        "    /** @return \\Generator<int, Customer> */\n",
        "    public function findAll(): \\Generator {}\n",
        "    public function process() {\n",
        "        foreach ($this->findAll() as $customer) {\n",
        "            $customer->\n",
        "        }\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 10, 24).await;
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(
        labels.iter().any(|l| l.starts_with("name")),
        "Should include 'name' from Customer via method returning Generator<int, Customer>. Got: {:?}",
        labels
    );
    assert!(
        labels.iter().any(|l| l.starts_with("getAddress")),
        "Should include 'getAddress' from Customer. Got: {:?}",
        labels
    );
}

/// Method returning `Generator` with four type parameters — value is still
/// the 2nd parameter.
#[tokio::test]
async fn test_foreach_generator_method_return_four_params() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///gen_method_four.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Task {\n",
        "    public string $title;\n",
        "    public function run(): void {}\n",
        "}\n",
        "class Result {\n",
        "    public bool $success;\n",
        "}\n",
        "class TaskRunner {\n",
        "    /** @return \\Generator<int, Task, mixed, Result> */\n",
        "    public function tasks(): \\Generator {}\n",
        "    public function execute() {\n",
        "        foreach ($this->tasks() as $task) {\n",
        "            $task->\n",
        "        }\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 13, 19).await;
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(
        labels.iter().any(|l| l.starts_with("title")),
        "Should include 'title' from Task (2nd param of 4-param Generator). Got: {:?}",
        labels
    );
    assert!(
        !labels.iter().any(|l| l.starts_with("success")),
        "Should NOT include 'success' from Result (TReturn, 4th param). Got: {:?}",
        labels
    );
}

// ─── Foreach over Generator from standalone function return type ─────────────

/// When a standalone function's `@return` specifies `Generator<int, User>`,
/// iterating the function call result should resolve the value variable.
#[tokio::test]
async fn test_foreach_generator_function_return_type() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///gen_func_ret.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Invoice {\n",
        "    public int $number;\n",
        "    public function send(): void {}\n",
        "}\n",
        "/** @return \\Generator<int, Invoice> */\n",
        "function generateInvoices(): \\Generator {}\n",
        "foreach (generateInvoices() as $invoice) {\n",
        "    $invoice->\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 8, 14).await;
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(
        labels.iter().any(|l| l.starts_with("number")),
        "Should include 'number' from Invoice via function returning Generator<int, Invoice>. Got: {:?}",
        labels
    );
    assert!(
        labels.iter().any(|l| l.starts_with("send")),
        "Should include 'send' from Invoice. Got: {:?}",
        labels
    );
}

// ─── Foreach over iterable<T> ───────────────────────────────────────────────

/// `iterable<User>` should resolve the foreach value variable to `User`.
#[tokio::test]
async fn test_foreach_iterable_single_param_var_annotation() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///iter_var_single.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class User {\n",
        "    public string $name;\n",
        "    public function getEmail(): string {}\n",
        "}\n",
        "class Handler {\n",
        "    public function handle() {\n",
        "        /** @var iterable<User> $items */\n",
        "        $items = $this->getItems();\n",
        "        foreach ($items as $item) {\n",
        "            $item->\n",
        "        }\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 10, 19).await;
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(
        labels.iter().any(|l| l.starts_with("name")),
        "Should include 'name' from User via iterable<User> foreach. Got: {:?}",
        labels
    );
}

/// `iterable<int, User>` should resolve the foreach value variable to `User`.
#[tokio::test]
async fn test_foreach_iterable_two_params_var_annotation() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///iter_var_two.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Order {\n",
        "    public int $id;\n",
        "    public function getTotal(): float {}\n",
        "}\n",
        "class Service {\n",
        "    public function process() {\n",
        "        /** @var iterable<int, Order> $orders */\n",
        "        $orders = $this->loadOrders();\n",
        "        foreach ($orders as $order) {\n",
        "            $order->\n",
        "        }\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 10, 20).await;
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(
        labels.iter().any(|l| l.starts_with("id")),
        "Should include 'id' from Order via iterable<int, Order> foreach. Got: {:?}",
        labels
    );
    assert!(
        labels.iter().any(|l| l.starts_with("getTotal")),
        "Should include 'getTotal' from Order. Got: {:?}",
        labels
    );
}

/// When a method returns `iterable<User>`, iterating should resolve value type.
#[tokio::test]
async fn test_foreach_iterable_method_return_type() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///iter_method_ret.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Product {\n",
        "    public string $title;\n",
        "    public function getPrice(): float {}\n",
        "}\n",
        "class Catalog {\n",
        "    /** @return iterable<Product> */\n",
        "    public function products(): iterable {}\n",
        "    public function display() {\n",
        "        foreach ($this->products() as $product) {\n",
        "            $product->\n",
        "        }\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 10, 22).await;
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(
        labels.iter().any(|l| l.starts_with("title")),
        "Should include 'title' from Product via method returning iterable<Product>. Got: {:?}",
        labels
    );
}

// ─── Foreach over Generator with @param annotation ──────────────────────────

/// When a method parameter is annotated with `@param Generator<int, User>`,
/// iterating it should resolve the value variable.
#[tokio::test]
async fn test_foreach_generator_param_annotation() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///gen_param.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Ticket {\n",
        "    public string $subject;\n",
        "    public function close(): void {}\n",
        "}\n",
        "class Processor {\n",
        "    /**\n",
        "     * @param \\Generator<int, Ticket> $tickets\n",
        "     */\n",
        "    public function process($tickets) {\n",
        "        foreach ($tickets as $ticket) {\n",
        "            $ticket->\n",
        "        }\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 11, 21).await;
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(
        labels.iter().any(|l| l.starts_with("subject")),
        "Should include 'subject' from Ticket via @param Generator<int, Ticket>. Got: {:?}",
        labels
    );
    assert!(
        labels.iter().any(|l| l.starts_with("close")),
        "Should include 'close' from Ticket. Got: {:?}",
        labels
    );
}

// ─── Generator with nested generic value types ──────────────────────────────

/// `Generator<int, Collection<string, Order>>` — the value type should be
/// `Collection<string, Order>`, which resolves to Collection and offers its
/// members.
#[tokio::test]
async fn test_foreach_generator_nested_generic_value() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///gen_nested.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Order {\n",
        "    public int $id;\n",
        "}\n",
        "class Collection {\n",
        "    public function first(): mixed {}\n",
        "    public function count(): int {}\n",
        "}\n",
        "class Batcher {\n",
        "    public function batches() {\n",
        "        /** @var \\Generator<int, Collection<string, Order>> $gen */\n",
        "        $gen = $this->getBatches();\n",
        "        foreach ($gen as $batch) {\n",
        "            $batch->\n",
        "        }\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 13, 20).await;
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(
        labels.iter().any(|l| l.starts_with("first")),
        "Should include 'first' from Collection via Generator<int, Collection<…>>. Got: {:?}",
        labels
    );
    assert!(
        labels.iter().any(|l| l.starts_with("count")),
        "Should include 'count' from Collection. Got: {:?}",
        labels
    );
}

// ─── Cross-file Generator resolution ────────────────────────────────────────

/// Generator yield type should resolve across files via PSR-4.
#[tokio::test]
async fn test_foreach_generator_cross_file() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{ "autoload": { "psr-4": { "App\\": "src/" } } }"#,
        &[(
            "src/Models/Article.php",
            concat!(
                "<?php\n",
                "namespace App\\Models;\n",
                "class Article {\n",
                "    public string $title;\n",
                "    public function getAuthor(): string {}\n",
                "}\n",
            ),
        )],
    );

    let uri = Url::parse("file:///gen_cross.php").unwrap();
    let text = concat!(
        "<?php\n",
        "use App\\Models\\Article;\n",
        "/** @var \\Generator<int, Article> $articles */\n",
        "$articles = loadArticles();\n",
        "foreach ($articles as $article) {\n",
        "    $article->\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 5, 14).await;
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(
        labels.iter().any(|l| l.starts_with("title")),
        "Should include 'title' from Article via cross-file Generator. Got: {:?}",
        labels
    );
    assert!(
        labels.iter().any(|l| l.starts_with("getAuthor")),
        "Should include 'getAuthor' from Article. Got: {:?}",
        labels
    );
}

// ─── Property chain on Generator foreach value ──────────────────────────────

/// Property chains on a foreach value from a Generator should work.
#[tokio::test]
async fn test_foreach_generator_property_chain() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///gen_chain.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Address {\n",
        "    public string $city;\n",
        "    public string $street;\n",
        "}\n",
        "class Employee {\n",
        "    public Address $address;\n",
        "}\n",
        "class Report {\n",
        "    public function generate() {\n",
        "        /** @var \\Generator<int, Employee> $employees */\n",
        "        $employees = $this->loadEmployees();\n",
        "        foreach ($employees as $emp) {\n",
        "            $emp->address->\n",
        "        }\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 13, 27).await;
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(
        labels.iter().any(|l| l.starts_with("city")),
        "Should include 'city' from Address via Generator foreach property chain. Got: {:?}",
        labels
    );
    assert!(
        labels.iter().any(|l| l.starts_with("street")),
        "Should include 'street' from Address. Got: {:?}",
        labels
    );
}

// ─── Iterable method return type with @param on method parameter ────────────

/// When a method parameter is typed as `iterable` with a docblock override,
/// the foreach value variable should resolve.
#[tokio::test]
async fn test_foreach_iterable_param_annotation() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///iter_param.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Event {\n",
        "    public string $name;\n",
        "    public function fire(): void {}\n",
        "}\n",
        "class Dispatcher {\n",
        "    /**\n",
        "     * @param iterable<Event> $events\n",
        "     */\n",
        "    public function dispatch(iterable $events) {\n",
        "        foreach ($events as $event) {\n",
        "            $event->\n",
        "        }\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 11, 20).await;
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(
        labels.iter().any(|l| l.starts_with("name")),
        "Should include 'name' from Event via @param iterable<Event>. Got: {:?}",
        labels
    );
    assert!(
        labels.iter().any(|l| l.starts_with("fire")),
        "Should include 'fire' from Event. Got: {:?}",
        labels
    );
}

// ─── Generator yield type inference inside generator bodies ─────────────────

/// When a method declares `@return Generator<int, User>` and the body
/// contains `yield $var`, the variable `$var` should be inferred as `User`
/// (the TValue type) even without an explicit assignment.
#[tokio::test]
async fn test_generator_yield_reverse_inference_tvalue() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///gen_yield_reverse.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class User {\n",
        "    public string $name;\n",
        "    public function getEmail(): string {}\n",
        "}\n",
        "class UserRepository {\n",
        "    /** @return \\Generator<int, User> */\n",
        "    public function findAll(): \\Generator {\n",
        "        yield $user;\n",
        "        $user->\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 9, 15).await;
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(
        labels.iter().any(|l| l.starts_with("name")),
        "Should include 'name' from User via reverse yield inference. Got: {:?}",
        labels
    );
    assert!(
        labels.iter().any(|l| l.starts_with("getEmail")),
        "Should include 'getEmail' from User via reverse yield inference. Got: {:?}",
        labels
    );
}

/// Same as above but with four Generator type parameters.
#[tokio::test]
async fn test_generator_yield_reverse_inference_four_params() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///gen_yield_reverse4.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Order {\n",
        "    public int $id;\n",
        "    public function getTotal(): float {}\n",
        "}\n",
        "class OrderRepo {\n",
        "    /** @return \\Generator<int, Order, mixed, void> */\n",
        "    public function getOrders(): \\Generator {\n",
        "        yield $order;\n",
        "        $order->\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 9, 16).await;
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(
        labels.iter().any(|l| l.starts_with("id")),
        "Should include 'id' from Order (TValue of Generator<int, Order, mixed, void>). Got: {:?}",
        labels
    );
}

/// When `yield $key => $var` is used and the return type is
/// `Generator<int, User>`, the value variable should resolve to User.
#[tokio::test]
async fn test_generator_yield_pair_reverse_inference() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///gen_yield_pair.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Product {\n",
        "    public string $title;\n",
        "}\n",
        "class ProductLoader {\n",
        "    /** @return \\Generator<int, Product> */\n",
        "    public function loadAll(): \\Generator {\n",
        "        yield 0 => $product;\n",
        "        $product->\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 8, 18).await;
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(
        labels.iter().any(|l| l.starts_with("title")),
        "Should include 'title' from Product via yield pair reverse inference. Got: {:?}",
        labels
    );
}

/// When `$sent = yield $value`, the variable `$sent` should be typed as
/// TSend (3rd parameter of Generator<TKey, TValue, TSend, TReturn>).
#[tokio::test]
async fn test_generator_yield_send_type() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///gen_yield_send.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Request {\n",
        "    public string $url;\n",
        "    public function getMethod(): string {}\n",
        "}\n",
        "class Processor {\n",
        "    /** @return \\Generator<int, string, Request, void> */\n",
        "    public function process(): \\Generator {\n",
        "        $request = yield 'ready';\n",
        "        $request->\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 9, 19).await;
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(
        labels.iter().any(|l| l.starts_with("url")),
        "Should include 'url' from Request via TSend type. Got: {:?}",
        labels
    );
    assert!(
        labels.iter().any(|l| l.starts_with("getMethod")),
        "Should include 'getMethod' from Request via TSend type. Got: {:?}",
        labels
    );
}

/// When Generator has only two params, TSend is not available — yield
/// assignment should produce no completions (no crash).
#[tokio::test]
async fn test_generator_yield_send_type_missing_tsend() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///gen_yield_send_no_tsend.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Emitter {\n",
        "    /** @return \\Generator<int, string> */\n",
        "    public function emit(): \\Generator {\n",
        "        $sent = yield 'hello';\n",
        "        $sent->\n",
        "    }\n",
        "}\n",
    );

    // No TSend parameter → no completions, but no crash either.
    let items = complete_at(&backend, &uri, text, 5, 15).await;
    // Should be empty or at least not crash.
    assert!(
        items.is_empty() || !items.iter().any(|i| i.label.starts_with("url")),
        "Should not produce completions when TSend is missing. Got: {:?}",
        items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
}

/// Reverse yield inference should work in top-level functions (not only methods).
#[tokio::test]
async fn test_generator_yield_reverse_inference_top_level_function() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///gen_yield_toplevel.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Customer {\n",
        "    public string $name;\n",
        "}\n",
        "/** @return \\Generator<int, Customer> */\n",
        "function generateCustomers(): \\Generator {\n",
        "    yield $customer;\n",
        "    $customer->\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 7, 16).await;
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(
        labels.iter().any(|l| l.starts_with("name")),
        "Should include 'name' from Customer via reverse yield inference in top-level function. Got: {:?}",
        labels
    );
}

/// When a variable IS assigned (e.g. `$user = new User()`), the explicit
/// assignment should take priority over generator yield inference.
#[tokio::test]
async fn test_generator_yield_explicit_assignment_takes_priority() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///gen_yield_priority.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class User {\n",
        "    public string $name;\n",
        "}\n",
        "class Admin {\n",
        "    public string $role;\n",
        "}\n",
        "class Service {\n",
        "    /** @return \\Generator<int, User> */\n",
        "    public function findAll(): \\Generator {\n",
        "        $admin = new Admin();\n",
        "        yield $admin;\n",
        "        $admin->\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 12, 16).await;
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    // The explicit `new Admin()` assignment should win over the
    // Generator<int, User> yield inference.
    assert!(
        labels.iter().any(|l| l.starts_with("role")),
        "Explicit assignment to Admin should take priority. Got: {:?}",
        labels
    );
    assert!(
        !labels.iter().any(|l| l.starts_with("name")),
        "Should NOT include 'name' from User when Admin is explicitly assigned. Got: {:?}",
        labels
    );
}

/// Cross-file generator yield inference: the yielded type is defined in
/// another file loaded via PSR-4.
#[tokio::test]
async fn test_generator_yield_reverse_inference_cross_file() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{ "autoload": { "psr-4": { "App\\": "src/" } } }"#,
        &[(
            "src/Models/Invoice.php",
            concat!(
                "<?php\n",
                "namespace App\\Models;\n",
                "class Invoice {\n",
                "    public int $number;\n",
                "    public function getAmount(): float {}\n",
                "}\n",
            ),
        )],
    );

    let uri = Url::parse("file:///gen_yield_cross.php").unwrap();
    let text = concat!(
        "<?php\n",
        "use App\\Models\\Invoice;\n",
        "class InvoiceGenerator {\n",
        "    /** @return \\Generator<int, Invoice> */\n",
        "    public function generate(): \\Generator {\n",
        "        yield $invoice;\n",
        "        $invoice->\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 6, 19).await;
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(
        labels.iter().any(|l| l.starts_with("number")),
        "Should include 'number' from Invoice via cross-file yield inference. Got: {:?}",
        labels
    );
    assert!(
        labels.iter().any(|l| l.starts_with("getAmount")),
        "Should include 'getAmount' from Invoice via cross-file yield inference. Got: {:?}",
        labels
    );
}

// ─── Generator yield edge-case tests (from todo.md §33a) ───────────────────

/// When `yield $var` appears inside an `if` block and the cursor is
/// *inside* the same block (not after it), the variable should still
/// resolve via reverse yield inference.
#[tokio::test]
async fn test_generator_yield_inside_if_block_cursor_inside() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///gen_yield_if_inside.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class User {\n",
        "    public string $name;\n",
        "    public function getEmail(): string {}\n",
        "}\n",
        "class UserRepository {\n",
        "    /** @return \\Generator<int, User> */\n",
        "    public function filteredUsers(): \\Generator {\n",
        "        if (true) {\n",
        "            yield $user;\n",
        "            $user->\n",
        "        }\n",
        "    }\n",
        "}\n",
    );

    // Cursor is on line 10 (0-based), after `$user->`
    let items = complete_at(&backend, &uri, text, 10, 19).await;
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(
        labels.iter().any(|l| l.starts_with("name")),
        "Should include 'name' from User when yield and cursor are inside if block. Got: {:?}",
        labels
    );
    assert!(
        labels.iter().any(|l| l.starts_with("getEmail")),
        "Should include 'getEmail' from User when yield and cursor are inside if block. Got: {:?}",
        labels
    );
}

/// When `yield $var` appears inside an `if` block but the cursor is
/// *after* the block, reverse yield inference should still work.
#[tokio::test]
async fn test_generator_yield_inside_if_block_cursor_after() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///gen_yield_if_after.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class User {\n",
        "    public string $name;\n",
        "    public function getEmail(): string {}\n",
        "}\n",
        "class UserRepository {\n",
        "    /** @return \\Generator<int, User> */\n",
        "    public function filteredUsers(): \\Generator {\n",
        "        if (true) {\n",
        "            yield $user;\n",
        "        }\n",
        "        $user->\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 11, 15).await;
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(
        labels.iter().any(|l| l.starts_with("name")),
        "Should include 'name' from User when yield is inside if block and cursor is after. Got: {:?}",
        labels
    );
}

/// When `yield $var` appears inside a `foreach` loop body and the cursor
/// is inside the loop, the variable should still resolve.
#[tokio::test]
async fn test_generator_yield_inside_foreach_cursor_inside() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///gen_yield_foreach_in.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class User {\n",
        "    public string $name;\n",
        "}\n",
        "class UserRepository {\n",
        "    /** @return \\Generator<int, User> */\n",
        "    public function allUsers(): \\Generator {\n",
        "        foreach ($items as $item) {\n",
        "            yield $user;\n",
        "            $user->\n",
        "        }\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 9, 19).await;
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(
        labels.iter().any(|l| l.starts_with("name")),
        "Should include 'name' from User when yield and cursor are inside foreach. Got: {:?}",
        labels
    );
}

/// When two separate variables are yielded in the same generator,
/// the first variable should resolve (cursor after first yield, before second).
#[tokio::test]
async fn test_generator_yield_multiple_vars_first() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///gen_yield_multi_first.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class User {\n",
        "    public string $name;\n",
        "    public function getEmail(): string {}\n",
        "}\n",
        "class UserRepository {\n",
        "    /** @return \\Generator<int, User> */\n",
        "    public function allUsers(): \\Generator {\n",
        "        yield $first;\n",
        "        $first->\n",
        "        yield $second;\n",
        "        $second->getEmail();\n",
        "    }\n",
        "}\n",
    );

    // Cursor on `$first->` (line 9, col 16)
    let items = complete_at(&backend, &uri, text, 9, 16).await;
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(
        labels.iter().any(|l| l.starts_with("name")),
        "Should include 'name' from User for $first when multiple yields exist. Got: {:?}",
        labels
    );
}

/// When two separate variables are yielded, the second variable should
/// also resolve.
#[tokio::test]
async fn test_generator_yield_multiple_vars_second() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///gen_yield_multi_second.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class User {\n",
        "    public string $name;\n",
        "    public function getEmail(): string {}\n",
        "}\n",
        "class UserRepository {\n",
        "    /** @return \\Generator<int, User> */\n",
        "    public function allUsers(): \\Generator {\n",
        "        yield $first;\n",
        "        $first->getEmail();\n",
        "        yield $second;\n",
        "        $second->\n",
        "    }\n",
        "}\n",
    );

    // Cursor on `$second->` (line 11, col 17)
    let items = complete_at(&backend, &uri, text, 11, 17).await;
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(
        labels.iter().any(|l| l.starts_with("name")),
        "Should include 'name' from User for $second when multiple yields exist. Got: {:?}",
        labels
    );
    assert!(
        labels.iter().any(|l| l.starts_with("getEmail")),
        "Should include 'getEmail' from User for $second when multiple yields exist. Got: {:?}",
        labels
    );
}

/// Chaining a method call on a yield-inferred variable should resolve
/// the next link.  `$user->getProfile()->` should complete with Profile members.
#[tokio::test]
async fn test_generator_yield_chain_method_call() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///gen_yield_chain.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Profile {\n",
        "    public string $bio;\n",
        "}\n",
        "class User {\n",
        "    public string $name;\n",
        "    /** @return Profile */\n",
        "    public function getProfile(): Profile {}\n",
        "}\n",
        "class UserRepository {\n",
        "    /** @return \\Generator<int, User> */\n",
        "    public function withProfiles(): \\Generator {\n",
        "        yield $user;\n",
        "        $user->getProfile()->\n",
        "    }\n",
        "}\n",
    );

    // Cursor after `$user->getProfile()->`  (line 13, col 30)
    let items = complete_at(&backend, &uri, text, 13, 30).await;
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(
        labels.iter().any(|l| l.starts_with("bio")),
        "Should include 'bio' from Profile via chaining on yield-inferred User. Got: {:?}",
        labels
    );
}

/// Chaining a property access on a yield-inferred variable.
/// `$user->profile->` should complete with the Profile property's type members.
#[tokio::test]
async fn test_generator_yield_chain_property() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///gen_yield_chain_prop.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Profile {\n",
        "    public string $bio;\n",
        "}\n",
        "class User {\n",
        "    public Profile $profile;\n",
        "}\n",
        "class UserRepository {\n",
        "    /** @return \\Generator<int, User> */\n",
        "    public function withProfiles(): \\Generator {\n",
        "        yield $user;\n",
        "        $user->profile->\n",
        "    }\n",
        "}\n",
    );

    // Cursor after `$user->profile->`  (line 11, col 25)
    let items = complete_at(&backend, &uri, text, 11, 25).await;
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(
        labels.iter().any(|l| l.starts_with("bio")),
        "Should include 'bio' from Profile via property chain on yield-inferred User. Got: {:?}",
        labels
    );
}

/// TSend inference (`$var = yield`) should work when the target method
/// appears after other classes/methods with nested braces, which can
/// confuse the backward brace scan in `find_enclosing_return_type`.
#[tokio::test]
async fn test_generator_tsend_after_multiple_classes_with_braces() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///gen_tsend_multi_class.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Command {\n",
        "    public string $action;\n",
        "    public function execute(): void {}\n",
        "}\n",
        "class ServiceA {\n",
        "    public function doSomething(): void {\n",
        "        if (true) {\n",
        "            $x = 1;\n",
        "        }\n",
        "    }\n",
        "    public function doMore(): void {\n",
        "        foreach ([1,2] as $v) {\n",
        "            $y = $v;\n",
        "        }\n",
        "    }\n",
        "}\n",
        "class ServiceB {\n",
        "    public function helper(): string {\n",
        "        return 'ok';\n",
        "    }\n",
        "}\n",
        "class Worker {\n",
        "    /** @return \\Generator<int, string, Command, void> */\n",
        "    public function run(): \\Generator {\n",
        "        $cmd = yield 'ready';\n",
        "        $cmd->\n",
        "    }\n",
        "}\n",
    );

    // Cursor on `$cmd->` (line 26, col 14)
    let items = complete_at(&backend, &uri, text, 26, 14).await;
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(
        labels.iter().any(|l| l.starts_with("action")),
        "Should include 'action' from Command via TSend after multiple classes. Got: {:?}",
        labels
    );
    assert!(
        labels.iter().any(|l| l.starts_with("execute")),
        "Should include 'execute' from Command via TSend after multiple classes. Got: {:?}",
        labels
    );
}

/// TSend inference should work inside deeply nested control flow within the
/// generator method body.
#[tokio::test]
async fn test_generator_tsend_inside_nested_control_flow() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///gen_tsend_nested_flow.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Task {\n",
        "    public int $priority;\n",
        "}\n",
        "class Scheduler {\n",
        "    /** @return \\Generator<int, string, Task, void> */\n",
        "    public function schedule(): \\Generator {\n",
        "        while (true) {\n",
        "            if (true) {\n",
        "                $task = yield 'waiting';\n",
        "                $task->\n",
        "            }\n",
        "        }\n",
        "    }\n",
        "}\n",
    );

    // Cursor on `$task->` (line 10, col 23 — right after `->`)
    let items = complete_at(&backend, &uri, text, 10, 23).await;
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(
        labels.iter().any(|l| l.starts_with("priority")),
        "Should include 'priority' from Task via TSend inside nested control flow. Got: {:?}",
        labels
    );
}
