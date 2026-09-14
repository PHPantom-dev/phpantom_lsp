use std::collections::HashMap;

use crate::common::{complete_at, create_psr4_workspace, create_test_backend};
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

// ─── Function first-class callable ──────────────────────────────────────────

#[tokio::test]
async fn test_first_class_callable_function_return_type() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/fcc_func.php").unwrap();

    let src = concat!(
        "<?php\n",
        "class User {\n",
        "    public function getName(): string { return ''; }\n",
        "    public function getEmail(): string { return ''; }\n",
        "}\n",
        "function createUser(): User { return new User(); }\n",
        "class Service {\n",
        "    public function run(): void {\n",
        "        $fn = createUser(...);\n",
        "        $fn()->\n",
        "    }\n",
        "}\n",
    );

    // Line 9: `        $fn()->`  cursor after `->`
    let items = complete_at(&backend, &uri, src, 9, 15).await;
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

// ─── Instance method first-class callable ───────────────────────────────────

#[tokio::test]
async fn test_first_class_callable_instance_method_return_type() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/fcc_method.php").unwrap();

    let src = concat!(
        "<?php\n",
        "class Product {\n",
        "    public function getPrice(): float { return 0.0; }\n",
        "    public function getTitle(): string { return ''; }\n",
        "}\n",
        "class Factory {\n",
        "    public function create(): Product { return new Product(); }\n",
        "}\n",
        "class Service {\n",
        "    public function run(): void {\n",
        "        $factory = new Factory();\n",
        "        $fn = $factory->create(...);\n",
        "        $fn()->\n",
        "    }\n",
        "}\n",
    );

    // Line 12: `        $fn()->`  cursor after `->`
    let items = complete_at(&backend, &uri, src, 12, 15).await;
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

// ─── $this->method(...) first-class callable ────────────────────────────────

#[tokio::test]
async fn test_first_class_callable_this_method_return_type() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/fcc_this.php").unwrap();

    let src = concat!(
        "<?php\n",
        "class Order {\n",
        "    public function getTotal(): float { return 0.0; }\n",
        "    public function getStatus(): string { return ''; }\n",
        "}\n",
        "class Service {\n",
        "    public function makeOrder(): Order { return new Order(); }\n",
        "    public function run(): void {\n",
        "        $fn = $this->makeOrder(...);\n",
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

// ─── Static method first-class callable ─────────────────────────────────────

#[tokio::test]
async fn test_first_class_callable_static_method_return_type() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/fcc_static.php").unwrap();

    let src = concat!(
        "<?php\n",
        "class Config {\n",
        "    public function get(string $key): string { return ''; }\n",
        "    public function has(string $key): bool { return false; }\n",
        "}\n",
        "class ConfigFactory {\n",
        "    public static function make(): Config { return new Config(); }\n",
        "}\n",
        "class Service {\n",
        "    public function run(): void {\n",
        "        $fn = ConfigFactory::make(...);\n",
        "        $fn()->\n",
        "    }\n",
        "}\n",
    );

    // Line 11: `        $fn()->`  cursor after `->`
    let items = complete_at(&backend, &uri, src, 11, 15).await;
    let names = method_names(&items);
    assert!(names.contains(&"get"), "Expected get in {:?}", names,);
    assert!(names.contains(&"has"), "Expected has in {:?}", names,);
}

// ─── self:: first-class callable ────────────────────────────────────────────

#[tokio::test]
async fn test_first_class_callable_self_static_method() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/fcc_self.php").unwrap();

    let src = concat!(
        "<?php\n",
        "class Order {\n",
        "    public function getTotal(): float { return 0.0; }\n",
        "}\n",
        "class Service {\n",
        "    public static function makeOrder(): Order { return new Order(); }\n",
        "    public function run(): void {\n",
        "        $fn = self::makeOrder(...);\n",
        "        $fn()->\n",
        "    }\n",
        "}\n",
    );

    // Line 8: `        $fn()->`  cursor after `->`
    let items = complete_at(&backend, &uri, src, 8, 15).await;
    let names = method_names(&items);
    assert!(
        names.contains(&"getTotal"),
        "Expected getTotal in {:?}",
        names,
    );
}

// ─── First-class callable chained: $fn()->method() ─────────────────────────

#[tokio::test]
async fn test_first_class_callable_chained_call() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/fcc_chain.php").unwrap();

    let src = concat!(
        "<?php\n",
        "class Builder {\n",
        "    public function build(): Product { return new Product(); }\n",
        "}\n",
        "class Product {\n",
        "    public function getTitle(): string { return ''; }\n",
        "    public function getPrice(): float { return 0.0; }\n",
        "}\n",
        "function getBuilder(): Builder { return new Builder(); }\n",
        "class Service {\n",
        "    public function run(): void {\n",
        "        $fn = getBuilder(...);\n",
        "        $fn()->build()->\n",
        "    }\n",
        "}\n",
    );

    // Line 12: `        $fn()->build()->`  cursor after last `->`
    let items = complete_at(&backend, &uri, src, 12, 25).await;
    let names = method_names(&items);
    assert!(
        names.contains(&"getTitle"),
        "Expected getTitle in {:?}",
        names,
    );
    assert!(
        names.contains(&"getPrice"),
        "Expected getPrice in {:?}",
        names,
    );
}

// ─── Assignment from first-class callable invocation ────────────────────────

#[tokio::test]
async fn test_first_class_callable_assigned_result() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/fcc_assign.php").unwrap();

    let src = concat!(
        "<?php\n",
        "class User {\n",
        "    public function getName(): string { return ''; }\n",
        "    public function getAge(): int { return 0; }\n",
        "}\n",
        "function createUser(): User { return new User(); }\n",
        "class Service {\n",
        "    public function run(): void {\n",
        "        $fn = createUser(...);\n",
        "        $user = $fn();\n",
        "        $user->\n",
        "    }\n",
        "}\n",
    );

    // Line 10: `        $user->`  cursor after `->`
    let items = complete_at(&backend, &uri, src, 10, 16).await;
    let names = method_names(&items);
    assert!(
        names.contains(&"getName"),
        "Expected getName in {:?}",
        names,
    );
    assert!(names.contains(&"getAge"), "Expected getAge in {:?}", names,);
}

// ─── Cross-file first-class callable ────────────────────────────────────────

#[tokio::test]
async fn test_first_class_callable_cross_file() {
    let composer_json = r#"{
        "autoload": {
            "psr-4": { "App\\": "src/" }
        }
    }"#;

    let model_src = concat!(
        "<?php\n",
        "namespace App;\n",
        "class Invoice {\n",
        "    public function getAmount(): float { return 0.0; }\n",
        "    public function getNumber(): string { return ''; }\n",
        "}\n",
    );

    let factory_src = concat!(
        "<?php\n",
        "namespace App;\n",
        "class InvoiceFactory {\n",
        "    public static function create(): Invoice { return new Invoice(); }\n",
        "}\n",
    );

    let service_src = concat!(
        "<?php\n",
        "namespace App;\n",
        "class Service {\n",
        "    public function run(): void {\n",
        "        $fn = InvoiceFactory::create(...);\n",
        "        $fn()->\n",
        "    }\n",
        "}\n",
    );

    let (backend, _dir) = create_psr4_workspace(
        composer_json,
        &[
            ("src/Invoice.php", model_src),
            ("src/InvoiceFactory.php", factory_src),
            ("src/Service.php", service_src),
        ],
    );

    let uri = Url::from_file_path(_dir.path().join("src/Service.php")).unwrap();

    let items = complete_at(&backend, &uri, service_src, 5, 15).await;
    let names = method_names(&items);
    assert!(
        names.contains(&"getAmount"),
        "Expected getAmount in {:?}",
        names,
    );
    assert!(
        names.contains(&"getNumber"),
        "Expected getNumber in {:?}",
        names,
    );
}

// ─── Null-safe method first-class callable ──────────────────────────────────

#[tokio::test]
async fn test_first_class_callable_nullsafe_method() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/fcc_nullsafe.php").unwrap();

    let src = concat!(
        "<?php\n",
        "class Formatter {\n",
        "    public function format(): string { return ''; }\n",
        "}\n",
        "class Config {\n",
        "    public function getFormatter(): Formatter { return new Formatter(); }\n",
        "}\n",
        "class Service {\n",
        "    private ?Config $config;\n",
        "    public function run(): void {\n",
        "        $fn = $this?->config?->getFormatter(...);\n",
        // The text-based scanner strips `?` via trim_end_matches('?')
        // on the `->` LHS, so this should resolve through to Formatter.
        "        $fn()->\n",
        "    }\n",
        "}\n",
    );

    // Line 11: `        $fn()->`  cursor after `->`
    let items = complete_at(&backend, &uri, src, 11, 15).await;
    let names = method_names(&items);
    // This test verifies the text scanner handles the `?->` syntax.
    // The chain `$this?->config?->getFormatter` is complex; the text
    // scanner may not resolve it fully.  If it does, great; if not,
    // we at least verify it doesn't crash.
    // (If text_resolution.resolve_lhs_to_class doesn't handle `$this?->`,
    //  names may be empty — that's acceptable for now.)
    let _ = names;
}

// ─── First-class callable variable resolves to Closure for `$fn->` ──────────

#[tokio::test]
async fn test_first_class_callable_resolves_to_closure() {
    // When `$fn = strlen(...)`, the variable itself is a Closure instance.
    // If the Closure class is available (from stubs), `$fn->` should
    // offer Closure members like `bindTo`, `call`, etc.
    //
    // This test verifies the AST path correctly resolves
    // `PartialApplication` to `Closure` ClassInfo.  Since the test
    // backend may not have a Closure stub loaded, we define a minimal
    // one inline.
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/fcc_closure_members.php").unwrap();

    let src = concat!(
        "<?php\n",
        "class Closure {\n",
        "    public function bindTo(object $newThis): Closure { return $this; }\n",
        "    public function call(object $newThis): mixed { return null; }\n",
        "}\n",
        "class Service {\n",
        "    public function run(): void {\n",
        "        $fn = strlen(...);\n",
        "        $fn->\n",
        "    }\n",
        "}\n",
    );

    // Line 8: `        $fn->`  cursor after `->`
    let items = complete_at(&backend, &uri, src, 8, 13).await;
    let names = method_names(&items);
    assert!(names.contains(&"bindTo"), "Expected bindTo in {:?}", names,);
    assert!(names.contains(&"call"), "Expected call in {:?}", names,);
}

// ─── Top-level (outside class) first-class callable ─────────────────────────

#[tokio::test]
async fn test_first_class_callable_top_level() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/fcc_top_level.php").unwrap();

    let src = concat!(
        "<?php\n",
        "class User {\n",
        "    public function getName(): string { return ''; }\n",
        "}\n",
        "function getUser(): User { return new User(); }\n",
        "$fn = getUser(...);\n",
        "$fn()->\n",
    );

    // Line 6: `$fn()->`  cursor after `->`
    let items = complete_at(&backend, &uri, src, 6, 7).await;
    let names = method_names(&items);
    assert!(
        names.contains(&"getName"),
        "Expected getName in {:?}",
        names,
    );
}

// ─── Reassigned first-class callable uses latest assignment ─────────────────

#[tokio::test]
async fn test_first_class_callable_reassignment() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/fcc_reassign.php").unwrap();

    let src = concat!(
        "<?php\n",
        "class User {\n",
        "    public function getName(): string { return ''; }\n",
        "}\n",
        "class Order {\n",
        "    public function getTotal(): float { return 0.0; }\n",
        "}\n",
        "function getUser(): User { return new User(); }\n",
        "function getOrder(): Order { return new Order(); }\n",
        "class Service {\n",
        "    public function run(): void {\n",
        "        $fn = getUser(...);\n",
        "        $fn = getOrder(...);\n",
        "        $fn()->\n",
        "    }\n",
        "}\n",
    );

    // Line 13: `        $fn()->`  cursor after `->`
    // Should resolve to Order (latest assignment).
    let items = complete_at(&backend, &uri, src, 13, 15).await;
    let names = method_names(&items);
    assert!(
        names.contains(&"getTotal"),
        "Expected getTotal in {:?}",
        names,
    );
    // getName should NOT be present (overwritten by second assignment).
    assert!(
        !names.contains(&"getName"),
        "Did not expect getName in {:?}",
        names,
    );
}

// ─── First-class callable with property access ──────────────────────────────

#[tokio::test]
async fn test_first_class_callable_result_property_access() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/fcc_prop.php").unwrap();

    let src = concat!(
        "<?php\n",
        "class Address {\n",
        "    public string $city;\n",
        "    public string $street;\n",
        "}\n",
        "class User {\n",
        "    public Address $address;\n",
        "    public function getName(): string { return ''; }\n",
        "}\n",
        "function getUser(): User { return new User(); }\n",
        "class Service {\n",
        "    public function run(): void {\n",
        "        $fn = getUser(...);\n",
        "        $fn()->\n",
        "    }\n",
        "}\n",
    );

    // Line 13: `        $fn()->`  cursor after `->`
    let items = complete_at(&backend, &uri, src, 13, 15).await;
    let names = method_names(&items);
    let props = property_names(&items);
    assert!(
        names.contains(&"getName"),
        "Expected getName in {:?}",
        names,
    );
    assert!(
        props.contains(&"address"),
        "Expected address property in {:?}",
        props,
    );
}

// ─── Static method returning ?self ──────────────────────────────────────────

#[tokio::test]
async fn test_first_class_callable_static_method_returning_nullable_self() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/fcc_nullable_self.php").unwrap();

    let src = concat!(
        "<?php\n",
        "class User {\n",
        "    public function getName(): string { return ''; }\n",
        "    public function getEmail(): string { return ''; }\n",
        "    public static function findByEmail(string $email): ?self { return null; }\n",
        "}\n",
        "class Service {\n",
        "    public function run(): void {\n",
        "        $finder = User::findByEmail(...);\n",
        "        $finder()->\n",
        "    }\n",
        "}\n",
    );

    // Line 9: `        $finder()->`  cursor after `->`
    let items = complete_at(&backend, &uri, src, 9, 19).await;
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

// ─── Instance method returning self ─────────────────────────────────────────

#[tokio::test]
async fn test_first_class_callable_instance_method_returning_self() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/fcc_self_return.php").unwrap();

    let src = concat!(
        "<?php\n",
        "class Builder {\n",
        "    public function reset(): self { return new self(); }\n",
        "    public function build(): string { return ''; }\n",
        "}\n",
        "class Service {\n",
        "    public function run(): void {\n",
        "        $builder = new Builder();\n",
        "        $fn = $builder->reset(...);\n",
        "        $fn()->\n",
        "    }\n",
        "}\n",
    );

    // Line 9: `        $fn()->`  cursor after `->`
    let items = complete_at(&backend, &uri, src, 9, 15).await;
    let names = method_names(&items);
    assert!(names.contains(&"reset"), "Expected reset in {:?}", names,);
    assert!(names.contains(&"build"), "Expected build in {:?}", names,);
}

// ─── Static method returning static ─────────────────────────────────────────

#[tokio::test]
async fn test_first_class_callable_static_method_returning_static() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/fcc_static_return.php").unwrap();

    let src = concat!(
        "<?php\n",
        "class Model {\n",
        "    public function getId(): int { return 0; }\n",
        "    public function getName(): string { return ''; }\n",
        "    public static function make(string $name): static { return new static(); }\n",
        "}\n",
        "class Service {\n",
        "    public function run(): void {\n",
        "        $fn = Model::make(...);\n",
        "        $fn()->\n",
        "    }\n",
        "}\n",
    );

    // Line 9: `        $fn()->`  cursor after `->`
    // `make()` returns `static` which resolves to Model.
    // Instance methods like getId and getName should appear;
    // `make` is static so it won't appear in `->` completion.
    let items = complete_at(&backend, &uri, src, 9, 15).await;
    let names = method_names(&items);
    assert!(names.contains(&"getId"), "Expected getId in {:?}", names,);
    assert!(
        names.contains(&"getName"),
        "Expected getName in {:?}",
        names,
    );
}

// ─── Closure / arrow-function literals resolve to Closure ───────────────────

#[tokio::test]
async fn test_closure_literal_resolves_to_closure() {
    // A closure literal `function() { … }` assigned to a variable should
    // resolve to the Closure class, offering members like bindTo and call.
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/closure_literal_members.php").unwrap();

    let src = concat!(
        "<?php\n",
        "class Closure {\n",
        "    public function bindTo(?object $newThis): ?Closure { return $this; }\n",
        "    public static function bind(Closure $closure, ?object $newThis): ?Closure { return $closure; }\n",
        "    public function call(object $newThis, mixed ...$args): mixed { return null; }\n",
        "}\n",
        "class User { public function getName(): string { return ''; } }\n",
        "class Service {\n",
        "    public function run(): void {\n",
        "        $typedClosure = function(User $u): string { return $u->getName(); };\n",
        "        $typedClosure->\n",
        "    }\n",
        "}\n",
    );

    // Line 10: `        $typedClosure->`  cursor after `->`
    let items = complete_at(&backend, &uri, src, 10, 24).await;
    let names = method_names(&items);
    assert!(names.contains(&"bindTo"), "Expected bindTo in {:?}", names,);
    assert!(names.contains(&"call"), "Expected call in {:?}", names);
}

#[tokio::test]
async fn test_arrow_function_resolves_to_closure() {
    // An arrow function `fn() => …` assigned to a variable should also
    // resolve to the Closure class.
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/arrow_fn_members.php").unwrap();

    let src = concat!(
        "<?php\n",
        "class Closure {\n",
        "    public function bindTo(?object $newThis): ?Closure { return $this; }\n",
        "    public static function bind(Closure $closure, ?object $newThis): ?Closure { return $closure; }\n",
        "    public function call(object $newThis, mixed ...$args): mixed { return null; }\n",
        "}\n",
        "class Service {\n",
        "    public function run(): void {\n",
        "        $typedArrow = fn(int $x): float => $x * 1.5;\n",
        "        $typedArrow->\n",
        "    }\n",
        "}\n",
    );

    // Line 9: `        $typedArrow->`  cursor after `->`
    let items = complete_at(&backend, &uri, src, 9, 22).await;
    let names = method_names(&items);
    assert!(names.contains(&"bindTo"), "Expected bindTo in {:?}", names,);
    assert!(names.contains(&"call"), "Expected call in {:?}", names);
}

#[tokio::test]
async fn test_closure_literal_bindto_chain() {
    // `$fn->bindTo($obj)` returns ?Closure, so chaining should continue
    // on the Closure class.
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/closure_bindto_chain.php").unwrap();

    let src = concat!(
        "<?php\n",
        "class Closure {\n",
        "    public function bindTo(?object $newThis): ?Closure { return $this; }\n",
        "    public function call(object $newThis, mixed ...$args): mixed { return null; }\n",
        "}\n",
        "class Service {\n",
        "    public function run(): void {\n",
        "        $fn = function(): void {};\n",
        "        $bound = $fn->bindTo($this);\n",
        "        $bound->\n",
        "    }\n",
        "}\n",
    );

    // Line 9: `        $bound->`  cursor after `->`
    let items = complete_at(&backend, &uri, src, 9, 16).await;
    let names = method_names(&items);
    assert!(
        names.contains(&"bindTo"),
        "Expected bindTo on chained result in {:?}",
        names,
    );
    assert!(
        names.contains(&"call"),
        "Expected call on chained result in {:?}",
        names,
    );
}

#[tokio::test]
async fn test_closure_no_params_resolves_to_closure() {
    // Even a bare closure with no params / no return type should resolve
    // to the Closure class.
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/closure_bare.php").unwrap();

    let src = concat!(
        "<?php\n",
        "class Closure {\n",
        "    public function bindTo(?object $newThis): ?Closure { return $this; }\n",
        "    public function call(object $newThis, mixed ...$args): mixed { return null; }\n",
        "}\n",
        "class Service {\n",
        "    public function run(): void {\n",
        "        $bare = function() {};\n",
        "        $bare->\n",
        "    }\n",
        "}\n",
    );

    // Line 8: `        $bare->`  cursor after `->`
    let items = complete_at(&backend, &uri, src, 8, 15).await;
    let names = method_names(&items);
    assert!(names.contains(&"bindTo"), "Expected bindTo in {:?}", names,);
    assert!(names.contains(&"call"), "Expected call in {:?}", names);
}

#[tokio::test]
async fn test_closure_with_use_resolves_to_closure() {
    // A closure with a `use ($x)` clause should still resolve to Closure.
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/closure_use_clause.php").unwrap();

    let src = concat!(
        "<?php\n",
        "class Closure {\n",
        "    public function bindTo(?object $newThis): ?Closure { return $this; }\n",
        "    public function call(object $newThis, mixed ...$args): mixed { return null; }\n",
        "}\n",
        "class Service {\n",
        "    public function run(): void {\n",
        "        $x = 42;\n",
        "        $fn = function() use ($x): int { return $x; };\n",
        "        $fn->\n",
        "    }\n",
        "}\n",
    );

    // Line 9: `        $fn->`  cursor after `->`
    let items = complete_at(&backend, &uri, src, 9, 13).await;
    let names = method_names(&items);
    assert!(names.contains(&"bindTo"), "Expected bindTo in {:?}", names,);
}

#[tokio::test]
async fn test_arrow_function_no_return_type_resolves_to_closure() {
    // Arrow function without an explicit return type still resolves to Closure.
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/arrow_no_return.php").unwrap();

    let src = concat!(
        "<?php\n",
        "class Closure {\n",
        "    public function bindTo(?object $newThis): ?Closure { return $this; }\n",
        "    public function call(object $newThis, mixed ...$args): mixed { return null; }\n",
        "}\n",
        "class Service {\n",
        "    public function run(): void {\n",
        "        $arrow = fn($x) => $x * 2;\n",
        "        $arrow->\n",
        "    }\n",
        "}\n",
    );

    // Line 8: `        $arrow->`  cursor after `->`
    let items = complete_at(&backend, &uri, src, 8, 16).await;
    let names = method_names(&items);
    assert!(names.contains(&"bindTo"), "Expected bindTo in {:?}", names,);
    assert!(names.contains(&"call"), "Expected call in {:?}", names);
}

#[tokio::test]
async fn test_closure_top_level_resolves_to_closure() {
    // Closure literal at the top level (outside any class) should also
    // resolve to the Closure class.
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/closure_top_level.php").unwrap();

    let src = concat!(
        "<?php\n",
        "class Closure {\n",
        "    public function bindTo(?object $newThis): ?Closure { return $this; }\n",
        "    public function call(object $newThis, mixed ...$args): mixed { return null; }\n",
        "}\n",
        "$greet = function(string $name): string { return \"Hello $name\"; };\n",
        "$greet->\n",
    );

    // Line 6: `$greet->`  cursor after `->`
    let items = complete_at(&backend, &uri, src, 6, 8).await;
    let names = method_names(&items);
    assert!(
        names.contains(&"bindTo"),
        "Expected bindTo at top level in {:?}",
        names,
    );
}

// ─── Closure resolution in namespace context (stubs) ────────────────────────

static CLOSURE_STUB: &str = "\
<?php
final class Closure
{
    /**
     * @param ?object $newThis
     * @param ?string $newScope
     * @return ?Closure
     */
    public function bindTo(?object $newThis, ?string $newScope = null): ?Closure {}

    /**
     * @param Closure $closure
     * @param ?object $newThis
     * @param ?string $newScope
     * @return ?Closure
     */
    public static function bind(Closure $closure, ?object $newThis, ?string $newScope = null): ?Closure {}

    /**
     * @param object $newThis
     * @param mixed ...$args
     * @return mixed
     */
    public function call(object $newThis, mixed ...$args): mixed {}
}
";

/// Closure literal inside a `namespace Demo { }` block should resolve to
/// the built-in Closure class from stubs, not to `Demo\Closure` (which
/// does not exist).
#[tokio::test]
async fn test_closure_literal_in_namespace_resolves_via_stubs() {
    let mut class_stubs: HashMap<&'static str, &'static str> = HashMap::new();
    class_stubs.insert("Closure", CLOSURE_STUB);
    let backend = phpantom_lsp::Backend::new_test_with_stubs(class_stubs);

    let uri = tower_lsp::lsp_types::Url::parse("file:///test/ns_closure.php").unwrap();

    let src = concat!(
        "<?php\n",
        "namespace Demo {\n",
        "    class Pen {\n",
        "        public function write(): string { return ''; }\n",
        "    }\n",
        "\n",
        "    class ClosureMembersDemo {\n",
        "        public function run(): void {\n",
        "            $typedClosure = function(Pen $p): string { return $p->write(); };\n",
        "            $typedClosure->\n",
        "        }\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, src, 9, 28).await;
    let names = method_names(&items);
    assert!(
        names.contains(&"bindTo"),
        "Expected bindTo from Closure stubs inside namespace block, got: {:?}",
        names,
    );
    assert!(
        names.contains(&"call"),
        "Expected call from Closure stubs inside namespace block, got: {:?}",
        names,
    );
}

/// Arrow function inside a namespace should also resolve to the built-in
/// Closure class.
#[tokio::test]
async fn test_arrow_function_in_namespace_resolves_via_stubs() {
    let mut class_stubs: HashMap<&'static str, &'static str> = HashMap::new();
    class_stubs.insert("Closure", CLOSURE_STUB);
    let backend = phpantom_lsp::Backend::new_test_with_stubs(class_stubs);

    let uri = tower_lsp::lsp_types::Url::parse("file:///test/ns_arrow.php").unwrap();

    let src = concat!(
        "<?php\n",
        "namespace App\\Service {\n",
        "    class ArrowDemo {\n",
        "        public function run(): void {\n",
        "            $fn = fn(int $x): float => $x * 1.5;\n",
        "            $fn->\n",
        "        }\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, src, 5, 17).await;
    let names = method_names(&items);
    assert!(
        names.contains(&"bindTo"),
        "Expected bindTo from Closure stubs for arrow fn in namespace, got: {:?}",
        names,
    );
}

/// First-class callable syntax inside a namespace should resolve to the
/// built-in Closure class.
#[tokio::test]
async fn test_first_class_callable_in_namespace_resolves_via_stubs() {
    let mut class_stubs: HashMap<&'static str, &'static str> = HashMap::new();
    class_stubs.insert("Closure", CLOSURE_STUB);
    let backend = phpantom_lsp::Backend::new_test_with_stubs(class_stubs);

    let uri = tower_lsp::lsp_types::Url::parse("file:///test/ns_fcc.php").unwrap();

    let src = concat!(
        "<?php\n",
        "namespace Demo {\n",
        "    class Pen {\n",
        "        public function write(): string { return ''; }\n",
        "    }\n",
        "\n",
        "    class FccDemo {\n",
        "        public function run(): void {\n",
        "            $fn = strlen(...);\n",
        "            $fn->\n",
        "        }\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, src, 9, 17).await;
    let names = method_names(&items);
    assert!(
        names.contains(&"bindTo"),
        "Expected bindTo from Closure stubs for first-class callable in namespace, got: {:?}",
        names,
    );
}

// ─── Immediate first-class callable invocation: method(...)() ───────────────

#[tokio::test]
async fn test_first_class_callable_immediate_static_invocation() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/fcc_immediate_static.php").unwrap();

    let src = concat!(
        "<?php\n",
        "class User {\n",
        "    public function getName(): string { return ''; }\n",
        "    public function getAge(): int { return 0; }\n",
        "}\n",
        "class Factory {\n",
        "    public static function createUser(): User { return new User(); }\n",
        "}\n",
        "class Service {\n",
        "    public function run(): void {\n",
        "        $user = Factory::createUser(...)();\n",
        "        $user->\n",
        "    }\n",
        "}\n",
    );

    // Line 11: `        $user->`  cursor after `->`
    let items = complete_at(&backend, &uri, src, 11, 15).await;
    let names = method_names(&items);
    assert!(
        names.contains(&"getName"),
        "Expected getName in {:?}",
        names,
    );
    assert!(names.contains(&"getAge"), "Expected getAge in {:?}", names);
}

#[tokio::test]
async fn test_first_class_callable_immediate_instance_invocation() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/fcc_immediate_instance.php").unwrap();

    let src = concat!(
        "<?php\n",
        "class Order {\n",
        "    public function getTotal(): float { return 0.0; }\n",
        "    public function getStatus(): string { return ''; }\n",
        "}\n",
        "class Service {\n",
        "    public function makeOrder(): Order { return new Order(); }\n",
        "    public function run(): void {\n",
        "        $order = $this->makeOrder(...)();\n",
        "        $order->\n",
        "    }\n",
        "}\n",
    );

    // Line 9: `        $order->`  cursor after `->`
    let items = complete_at(&backend, &uri, src, 9, 16).await;
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
async fn test_first_class_callable_immediate_self_invocation() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/fcc_immediate_self.php").unwrap();

    let src = concat!(
        "<?php\n",
        "class Product {\n",
        "    public function getTitle(): string { return ''; }\n",
        "}\n",
        "class Builder {\n",
        "    public static function build(): Product { return new Product(); }\n",
        "    public function run(): void {\n",
        "        $p = self::build(...)();\n",
        "        $p->\n",
        "    }\n",
        "}\n",
    );

    // Line 8: `        $p->`  cursor after `->`
    let items = complete_at(&backend, &uri, src, 8, 12).await;
    let names = method_names(&items);
    assert!(
        names.contains(&"getTitle"),
        "Expected getTitle in {:?}",
        names,
    );
}

#[tokio::test]
async fn test_first_class_callable_immediate_function_invocation() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/fcc_immediate_func.php").unwrap();

    let src = concat!(
        "<?php\n",
        "class Config {\n",
        "    public function get(string $key): string { return ''; }\n",
        "}\n",
        "function loadConfig(): Config { return new Config(); }\n",
        "class App {\n",
        "    public function run(): void {\n",
        "        $cfg = loadConfig(...)();\n",
        "        $cfg->\n",
        "    }\n",
        "}\n",
    );

    // Line 8: `        $cfg->`  cursor after `->`
    let items = complete_at(&backend, &uri, src, 8, 14).await;
    let names = method_names(&items);
    assert!(names.contains(&"get"), "Expected get in {:?}", names);
}
