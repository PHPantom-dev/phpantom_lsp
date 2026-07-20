use crate::common::create_test_backend;
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

/// Helper: open a document and request completion at the given line/character.
async fn complete_at(
    backend: &phpantom_lsp::Backend,
    uri: &Url,
    text: &str,
    line: u32,
    character: u32,
) -> Option<CompletionResponse> {
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

    backend.completion(completion_params).await.unwrap()
}

fn assert_has_member(items: &[CompletionItem], member: &str) {
    let names: Vec<&str> = items
        .iter()
        .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
        .collect();
    assert!(
        names.contains(&member),
        "Should suggest '{}', got: {:?}",
        member,
        names
    );
}

fn unwrap_items(response: Option<CompletionResponse>) -> Vec<CompletionItem> {
    match response.expect("Should return completion results") {
        CompletionResponse::Array(items) => items,
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

// ─── Case 1a: /** @var array<int, Customer> $thing */ $thing = []; $thing[0]-> ──

#[tokio::test]
async fn test_var_array_int_customer_named_annotation() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test_arr_named.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Customer {\n",
        "    public string $name;\n",
        "    public function getEmail(): string {}\n",
        "}\n",
        "/** @var array<int, Customer> $thing */\n",
        "$thing = [];\n",
        "$thing[0]->\n",
    );

    let result = complete_at(&backend, &uri, text, 7, 11).await;
    let items = unwrap_items(result);
    assert_has_member(&items, "name");
    assert_has_member(&items, "getEmail");
}

// ─── Case 1b: /** @var array<int, Customer> */ $thing = []; $thing[0]-> ─────
// No variable name in the annotation — applies to the next assignment line.

#[tokio::test]
async fn test_var_array_int_customer_no_varname_annotation() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test_arr_no_varname.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Customer {\n",
        "    public string $name;\n",
        "    public function getEmail(): string {}\n",
        "}\n",
        "/** @var array<int, Customer> */\n",
        "$thing = [];\n",
        "$thing[0]->\n",
    );

    let result = complete_at(&backend, &uri, text, 7, 11).await;
    let items = unwrap_items(result);
    assert_has_member(&items, "name");
    assert_has_member(&items, "getEmail");
}

// ─── Case 1c: /** @var array<int, Customer> */ $thing = []; $thing[0]-> ──────

#[tokio::test]
async fn test_var_array_int_customer_empty_array_access() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test_arr_int_cust.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Customer {\n",
        "    public string $name;\n",
        "    public function getEmail(): string {}\n",
        "}\n",
        "/** @var array<int, Customer> $thing */\n",
        "$thing = getUnknownValue();\n",
        "$thing[0]->\n",
    );

    let result = complete_at(&backend, &uri, text, 7, 11).await;
    let items = unwrap_items(result);
    assert_has_member(&items, "name");
    assert_has_member(&items, "getEmail");
}

// ─── Case 2: /** @var array<Customer> */ $thing = []; $thing[0]-> ───────────

#[tokio::test]
async fn test_var_array_single_param_customer_access() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test_arr_single_cust.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Customer {\n",
        "    public string $name;\n",
        "    public function getEmail(): string {}\n",
        "}\n",
        "/** @var array<Customer> $thing */\n",
        "$thing = [];\n",
        "$thing[0]->\n",
    );

    let result = complete_at(&backend, &uri, text, 7, 11).await;
    let items = unwrap_items(result);
    assert_has_member(&items, "name");
    assert_has_member(&items, "getEmail");
}

// ─── Case 3a: /** @var list<Customer> $thing */ $thing = []; $thing[0]-> ────

#[tokio::test]
async fn test_var_list_customer_access() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test_list_cust.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Customer {\n",
        "    public string $name;\n",
        "    public function getEmail(): string {}\n",
        "}\n",
        "/** @var list<Customer> $thing */\n",
        "$thing = [];\n",
        "$thing[0]->\n",
    );

    let result = complete_at(&backend, &uri, text, 7, 11).await;
    let items = unwrap_items(result);
    assert_has_member(&items, "name");
    assert_has_member(&items, "getEmail");
}

// ─── Case 3b: /** @var list<Customer> */ $thing = []; $thing[0]-> ───────────
// No variable name in the annotation.

#[tokio::test]
async fn test_var_list_customer_no_varname_access() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test_list_cust_novar.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Customer {\n",
        "    public string $name;\n",
        "    public function getEmail(): string {}\n",
        "}\n",
        "/** @var list<Customer> */\n",
        "$thing = [];\n",
        "$thing[0]->\n",
    );

    let result = complete_at(&backend, &uri, text, 7, 11).await;
    let items = unwrap_items(result);
    assert_has_member(&items, "name");
    assert_has_member(&items, "getEmail");
}

// ─── Case 4: $thing = [new Customer()]; $thing[0]-> ────────────────────────

#[tokio::test]
async fn test_inferred_array_new_object_access() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test_inferred_arr.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Customer {\n",
        "    public string $name;\n",
        "    public function getEmail(): string {}\n",
        "}\n",
        "$thing = [new Customer()];\n",
        "$thing[0]->\n",
    );

    let result = complete_at(&backend, &uri, text, 6, 11).await;
    let items = unwrap_items(result);
    assert_has_member(&items, "name");
    assert_has_member(&items, "getEmail");
}

// ─── Case 5: [Customer::first()][0]-> ──────────────────────────────────────

#[tokio::test]
async fn test_inline_array_literal_static_call_access() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test_inline_arr.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Customer {\n",
        "    public string $name;\n",
        "    public function getEmail(): string {}\n",
        "    /** @return static */\n",
        "    public static function first(): static {}\n",
        "}\n",
        "[Customer::first()][0]->\n",
    );

    let result = complete_at(&backend, &uri, text, 7, 24).await;
    let items = unwrap_items(result);
    assert_has_member(&items, "name");
    assert_has_member(&items, "getEmail");
}

// ─── Case 6: end(Customer::get()->all())-> ─────────────────────────────────

#[tokio::test]
async fn test_end_of_method_chain_returning_array() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test_end_chain.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Customer {\n",
        "    public string $name;\n",
        "    public function getEmail(): string {}\n",
        "    /** @return Collection<int, static> */\n",
        "    public static function get(): Collection {}\n",
        "}\n",
        "class Collection {\n",
        "    /** @return array<int, Customer> */\n",
        "    public function all(): array {}\n",
        "}\n",
        "end(Customer::get()->all())->\n",
    );

    let result = complete_at(&backend, &uri, text, 11, 29).await;
    let items = unwrap_items(result);
    assert_has_member(&items, "name");
    assert_has_member(&items, "getEmail");
}

// ─── Extra: variable assigned from end() ────────────────────────────────────

#[tokio::test]
async fn test_variable_assigned_from_end_array_generic() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test_end_assign.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Customer {\n",
        "    public string $name;\n",
        "    public function getEmail(): string {}\n",
        "}\n",
        "/** @var array<int, Customer> $customers */\n",
        "$customers = [];\n",
        "$last = end($customers);\n",
        "$last->\n",
    );

    let result = complete_at(&backend, &uri, text, 8, 7).await;
    let items = unwrap_items(result);
    assert_has_member(&items, "name");
    assert_has_member(&items, "getEmail");
}

// ─── Extra: @var without explicit assignment to getUnknownValue() ───────────
// This pattern is known to work — serves as a sanity/regression check.

#[tokio::test]
async fn test_var_array_generic_with_unknown_value_rhs() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test_arr_unknown_rhs.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Customer {\n",
        "    public string $name;\n",
        "    public function getEmail(): string {}\n",
        "}\n",
        "function getUnknownValue(): mixed { return null; }\n",
        "/** @var array<int, Customer> $thing */\n",
        "$thing = getUnknownValue();\n",
        "$thing[0]->\n",
    );

    let result = complete_at(&backend, &uri, text, 8, 11).await;
    let items = unwrap_items(result);
    assert_has_member(&items, "name");
    assert_has_member(&items, "getEmail");
}

// ═══════════════════════════════════════════════════════════════════════════
// Method return → array access: $c->items()[0]->
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_method_return_array_access_bracket_type() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test_method_arr.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Item {\n",
        "    public function getLabel(): string { return ''; }\n",
        "}\n",
        "class Collection {\n",
        "    /** @return Item[] */\n",
        "    public function items(): array { return []; }\n",
        "}\n",
        "class Consumer {\n",
        "    public function run(): void {\n",
        "        $c = new Collection();\n",
        "        $c->items()[0]->\n",
        "    }\n",
        "}\n",
    );

    let result = complete_at(&backend, &uri, text, 11, 24).await;
    let items = unwrap_items(result);
    assert_has_member(&items, "getLabel");
}

#[tokio::test]
async fn test_method_return_array_access_generic_type() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test_method_arr_generic.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Item {\n",
        "    public function getLabel(): string { return ''; }\n",
        "}\n",
        "class Collection {\n",
        "    /** @return array<int, Item> */\n",
        "    public function items(): array { return []; }\n",
        "}\n",
        "class Consumer {\n",
        "    public function run(): void {\n",
        "        $c = new Collection();\n",
        "        $c->items()[0]->\n",
        "    }\n",
        "}\n",
    );

    let result = complete_at(&backend, &uri, text, 11, 24).await;
    let items = unwrap_items(result);
    assert_has_member(&items, "getLabel");
}

#[tokio::test]
async fn test_static_method_return_array_access() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test_static_method_arr.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Item {\n",
        "    public function getLabel(): string { return ''; }\n",
        "}\n",
        "class Collection {\n",
        "    /** @return Item[] */\n",
        "    public static function all(): array { return []; }\n",
        "}\n",
        "class Consumer {\n",
        "    public function run(): void {\n",
        "        Collection::all()[0]->\n",
        "    }\n",
        "}\n",
    );

    let result = complete_at(&backend, &uri, text, 10, 30).await;
    let items = unwrap_items(result);
    assert_has_member(&items, "getLabel");
}

#[tokio::test]
async fn test_method_return_template_class_string_array_access() {
    // A method declared `@return T[]` whose `T` is bound from a
    // `class-string<T>` argument must resolve its element type from the
    // call-site argument when the result is indexed inline:
    // `$a->findChildrenOfType(Foo::class)[0]->`.
    let backend = create_test_backend();
    let uri = Url::parse("file:///test_tmpl_arr.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Node {\n",
        "    public function getParent(): ?Node { return null; }\n",
        "}\n",
        "class Attr extends Node {\n",
        "    public function attrName(): string { return ''; }\n",
        "}\n",
        "class Holder {\n",
        "    /**\n",
        "     * @template T of Node\n",
        "     * @param class-string<T> $type\n",
        "     * @return T[]\n",
        "     */\n",
        "    public function findChildrenOfType(string $type): array { return []; }\n",
        "}\n",
        "class Consumer {\n",
        "    public function run(Holder $a): void {\n",
        "        $a->findChildrenOfType(Attr::class)[0]->\n",
        "    }\n",
        "}\n",
    );

    let result = complete_at(&backend, &uri, text, 17, 48).await;
    let items = unwrap_items(result);
    assert_has_member(&items, "attrName");
}

#[tokio::test]
async fn test_method_return_list_array_access() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test_method_list_arr.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Item {\n",
        "    public function getLabel(): string { return ''; }\n",
        "}\n",
        "class Collection {\n",
        "    /** @return list<Item> */\n",
        "    public function items(): array { return []; }\n",
        "}\n",
        "class Consumer {\n",
        "    public function run(): void {\n",
        "        $c = new Collection();\n",
        "        $c->items()[0]->\n",
        "    }\n",
        "}\n",
    );

    let result = complete_at(&backend, &uri, text, 11, 24).await;
    let items = unwrap_items(result);
    assert_has_member(&items, "getLabel");
}

// ═══════════════════════════════════════════════════════════════════════════
// Array element type extraction from generic array property annotations
// ═══════════════════════════════════════════════════════════════════════════

// ─── Property typed as array<string, SomeClass> with bracket access ─────

#[tokio::test]
async fn test_property_generic_array_bracket_access() {
    // $this->cache[$key]-> should resolve to IntCollection members
    // when cache is typed as array<string, IntCollection>.
    let backend = create_test_backend();
    let uri = Url::parse("file:///test_prop_generic_arr.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class IntCollection {\n",
        "    public function contains(int $id): bool { return false; }\n",
        "    public function count(): int { return 0; }\n",
        "}\n",
        "class SalesCampaignGroup {\n",
        "    /** @var array<string, IntCollection> */\n",
        "    private array $cache = [];\n",
        "\n",
        "    public function check(string $key, int $id): bool {\n",
        "        return $this->cache[$key]->\n",
        "    }\n",
        "}\n",
    );

    let result = complete_at(&backend, &uri, text, 10, 39).await;
    let items = unwrap_items(result);
    assert_has_member(&items, "contains");
    assert_has_member(&items, "count");
}

// ─── Property typed as Collection<int, Model> with bracket access ───────

#[tokio::test]
async fn test_property_collection_generic_bracket_access() {
    // $model->translations[0]-> should resolve to Translation members
    // when translations is typed as Collection<int, Translation>.
    let backend = create_test_backend();
    let uri = Url::parse("file:///test_prop_collection_arr.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Translation {\n",
        "    public string $name;\n",
        "    public function getLocale(): string { return ''; }\n",
        "}\n",
        "/**\n",
        " * @template TKey\n",
        " * @template TValue\n",
        " */\n",
        "class Collection {\n",
        "    /** @return TValue */\n",
        "    public function first() {}\n",
        "}\n",
        "class Product {\n",
        "    /** @var Collection<int, Translation> */\n",
        "    public Collection $translations;\n",
        "\n",
        "    public function getTranslationName(): string {\n",
        "        return $this->translations[0]->\n",
        "    }\n",
        "}\n",
    );

    let result = complete_at(&backend, &uri, text, 18, 42).await;
    let items = unwrap_items(result);
    assert_has_member(&items, "name");
    assert_has_member(&items, "getLocale");
}

// ─── Variable typed as array<string, SomeClass> with bracket access ─────

#[tokio::test]
async fn test_variable_generic_array_bracket_access_var_annotation() {
    // /** @var array<string, Order> $orders */ $orders[$key]->
    // should resolve to Order members.
    let backend = create_test_backend();
    let uri = Url::parse("file:///test_var_generic_arr_key.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Order {\n",
        "    public int $id;\n",
        "    public function getTotal(): float { return 0.0; }\n",
        "}\n",
        "/** @var array<string, Order> $orders */\n",
        "$orders = [];\n",
        "$orders['abc']->\n",
    );

    let result = complete_at(&backend, &uri, text, 7, 16).await;
    let items = unwrap_items(result);
    assert_has_member(&items, "id");
    assert_has_member(&items, "getTotal");
}

// ─── Property typed as array<int, SomeClass> on non-$this object ────────

#[tokio::test]
async fn test_object_property_generic_array_bracket_access() {
    // $service->items[$i]-> where $service->items is array<int, Widget>
    let backend = create_test_backend();
    let uri = Url::parse("file:///test_obj_prop_generic_arr.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Widget {\n",
        "    public string $label;\n",
        "    public function render(): string { return ''; }\n",
        "}\n",
        "class WidgetService {\n",
        "    /** @var array<int, Widget> */\n",
        "    public array $items = [];\n",
        "}\n",
        "function test(WidgetService $service): void {\n",
        "    $service->items[0]->\n",
        "}\n",
    );

    let result = complete_at(&backend, &uri, text, 10, 24).await;
    let items = unwrap_items(result);
    assert_has_member(&items, "label");
    assert_has_member(&items, "render");
}

// ─── Property with string key bracket access and method chain ───────────

#[tokio::test]
async fn test_property_generic_array_bracket_access_then_method_chain() {
    // $this->cache[$key]->first()-> should chain through the element type.
    let backend = create_test_backend();
    let uri = Url::parse("file:///test_prop_arr_chain.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Result {\n",
        "    public string $value;\n",
        "}\n",
        "class ResultSet {\n",
        "    public function first(): Result { return new Result(); }\n",
        "}\n",
        "class Cache {\n",
        "    /** @var array<string, ResultSet> */\n",
        "    private array $data = [];\n",
        "\n",
        "    public function lookup(string $key): void {\n",
        "        $this->data[$key]->first()->\n",
        "    }\n",
        "}\n",
    );

    let result = complete_at(&backend, &uri, text, 12, 38).await;
    let items = unwrap_items(result);
    assert_has_member(&items, "value");
}

// ─── Property typed as array<string, SomeClass> with string-literal key ─

#[tokio::test]
async fn test_property_generic_array_string_literal_key_access() {
    // $this->cache['myKey']-> should resolve to IntCollection members
    // even when the bracket index is a string literal (not a variable).
    let backend = create_test_backend();
    let uri = Url::parse("file:///test_prop_generic_arr_strkey.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class IntCollection {\n",
        "    public function contains(int $id): bool { return false; }\n",
        "    public function count(): int { return 0; }\n",
        "}\n",
        "class CacheHolder {\n",
        "    /** @var array<string, IntCollection> */\n",
        "    private array $cache = [];\n",
        "\n",
        "    public function check(int $id): bool {\n",
        "        return $this->cache['myKey']->\n",
        "    }\n",
        "}\n",
    );

    let result = complete_at(&backend, &uri, text, 10, 41).await;
    let items = unwrap_items(result);
    assert_has_member(&items, "contains");
    assert_has_member(&items, "count");
}

// ═══════════════════════════════════════════════════════════════════════════
// Indexing an object implementing `ArrayAccess` uses `offsetGet`
// ═══════════════════════════════════════════════════════════════════════════

// ─── Native ArrayAccess implementer, no docblock generics at all ────────

#[tokio::test]
async fn test_array_access_object_offset_get_no_generics() {
    // $list[0]-> on a class that implements ArrayAccess natively (no
    // @implements ArrayAccess<K, V> docblock) should resolve through
    // offsetGet()'s declared return type.
    let backend = create_test_backend();
    let uri = Url::parse("file:///test_array_access_no_generics.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Node {\n",
        "    public function getImage(): string { return ''; }\n",
        "}\n",
        "class NodeList implements ArrayAccess {\n",
        "    public function offsetExists(mixed $offset): bool { return false; }\n",
        "    public function offsetGet(mixed $offset): Node { return new Node(); }\n",
        "    public function offsetSet(mixed $offset, mixed $value): void {}\n",
        "    public function offsetUnset(mixed $offset): void {}\n",
        "}\n",
        "$list = new NodeList();\n",
        "$list[0]->\n",
    );

    let result = complete_at(&backend, &uri, text, 11, 10).await;
    let items = unwrap_items(result);
    assert_has_member(&items, "getImage");
}

// ─── ArrayAccess<K, T> where T is the class's own unbound @template ─────

#[tokio::test]
async fn test_array_access_own_template_self_reference_resolves_to_bound() {
    // Mirrors pdepend's ASTArtifactList: `@template T of Node` combined
    // with `@implements ArrayAccess<int|string, T>` where `T` refers to
    // the class's own template parameter, not a parent's. With no
    // instantiation-site generic annotation, `T` must resolve to its
    // declared bound (`Node`) rather than leaking through as a literal
    // type named "T".
    let backend = create_test_backend();
    let uri = Url::parse("file:///test_array_access_own_template.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Node {\n",
        "    public function getImage(): string { return ''; }\n",
        "}\n",
        "/**\n",
        " * @template T of Node\n",
        " * @implements ArrayAccess<int|string, T>\n",
        " */\n",
        "class NodeList implements ArrayAccess {\n",
        "    public function offsetExists(mixed $offset): bool { return false; }\n",
        "    /** @return T */\n",
        "    public function offsetGet(mixed $offset) {}\n",
        "    public function offsetSet(mixed $offset, mixed $value): void {}\n",
        "    public function offsetUnset(mixed $offset): void {}\n",
        "}\n",
        "$list = new NodeList([]);\n",
        "$list[0]->\n",
    );

    let result = complete_at(&backend, &uri, text, 16, 10).await;
    let items = unwrap_items(result);
    assert_has_member(&items, "getImage");
}
