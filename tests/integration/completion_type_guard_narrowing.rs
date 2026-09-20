use crate::common::{complete_at, create_test_backend, method_names};
use tower_lsp::lsp_types::*;

// ── is_array narrowing with PHPDoc generic list ─────────────────────────

#[tokio::test]
async fn test_is_array_narrows_to_generic_list_element_in_foreach() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///is_array_generic.php").unwrap();
    // Reproduces a pattern where a native union `null|array|Request`
    // with a PHPDoc `@param null|list<Request>|Request`.  After `is_array()`,
    // `$request` should narrow to `list<Request>`, so iterating yields `Request`.
    let text = concat!(
        "<?php\n",
        "class Request {\n",
        "    public function jsonSerialize(): array { return []; }\n",
        "}\n",
        "class Svc {\n",
        "    /**\n",
        "     * @param null|list<Request>|Request $request\n",
        "     */\n",
        "    public function connect(null|array|Request $request): void {\n",
        "        if (is_array($request)) {\n",
        "            foreach ($request as $item) {\n",
        "                $item->\n",
        "            }\n",
        "        }\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 11, 23).await;
    let methods = method_names(&items);

    assert!(
        methods.contains(&"jsonSerialize"),
        "After is_array() narrowing, foreach element should be Request with jsonSerialize; got: {:?}",
        methods
    );
}

// ── is_array narrowing on parameter with native array hint ──────────────

#[tokio::test]
async fn test_is_array_narrows_union_keeps_array_branch() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///is_array_simple.php").unwrap();
    // `$input` is `string|array<int, Foo>|Foo`.  After `is_array($input)`,
    // only the `array<int, Foo>` branch survives.  Foreach yields `Foo`.
    let text = concat!(
        "<?php\n",
        "class Foo {\n",
        "    public function doFoo(): void {}\n",
        "}\n",
        "class Svc {\n",
        "    /**\n",
        "     * @param string|array<int, Foo>|Foo $input\n",
        "     */\n",
        "    public function handle(string|array|Foo $input): void {\n",
        "        if (is_array($input)) {\n",
        "            foreach ($input as $item) {\n",
        "                $item->\n",
        "            }\n",
        "        }\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 11, 23).await;
    let methods = method_names(&items);

    assert!(
        methods.contains(&"doFoo"),
        "After is_array() narrowing, foreach element should be Foo; got: {:?}",
        methods
    );
}

// ── \is_array narrowing on backslash-prefixed builtin call ──────────────

#[tokio::test]
async fn test_backslash_prefixed_is_array_narrows_union_keeps_array_branch() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///is_array_fqn.php").unwrap();
    // Same as `test_is_array_narrows_union_keeps_array_branch`, but the
    // guard call is written `\is_array($input)` (the style enforced by
    // PHP-CS-Fixer's `native_function_invocation` rule).
    let text = concat!(
        "<?php\n",
        "class Foo {\n",
        "    public function doFoo(): void {}\n",
        "}\n",
        "class Svc {\n",
        "    /**\n",
        "     * @param string|array<int, Foo>|Foo $input\n",
        "     */\n",
        "    public function handle(string|array|Foo $input): void {\n",
        "        if (\\is_array($input)) {\n",
        "            foreach ($input as $item) {\n",
        "                $item->\n",
        "            }\n",
        "        }\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 11, 23).await;
    let methods = method_names(&items);

    assert!(
        methods.contains(&"doFoo"),
        "After \\is_array() narrowing, foreach element should be Foo; got: {:?}",
        methods
    );
}

// ── is_array inverse narrows to non-array members ───────────────────────

#[tokio::test]
async fn test_is_array_inverse_narrows_to_class_in_else() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///is_array_inverse.php").unwrap();
    // After `if (is_array($x)) { ... } else { $x-> }`, only the non-array
    // members survive.  Here the docblock says `list<Item>|Item`, so the
    // else-body should see `Item`.
    let text = concat!(
        "<?php\n",
        "class Item {\n",
        "    public function name(): string { return ''; }\n",
        "}\n",
        "class Svc {\n",
        "    /**\n",
        "     * @param list<Item>|Item $data\n",
        "     */\n",
        "    public function process(array|Item $data): void {\n",
        "        if (is_array($data)) {\n",
        "            // array branch\n",
        "        } else {\n",
        "            $data->\n",
        "        }\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 12, 20).await;
    let methods = method_names(&items);

    assert!(
        methods.contains(&"name"),
        "In else-body of is_array(), variable should be narrowed to Item; got: {:?}",
        methods
    );
}

// ── is_array guard clause narrows after early return ─────────────────────

#[tokio::test]
async fn test_is_array_guard_clause_narrows_to_class() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///is_array_guard.php").unwrap();
    // `if (is_array($x)) { return; }` — after the if, $x is NOT array.
    let text = concat!(
        "<?php\n",
        "class Widget {\n",
        "    public function render(): void {}\n",
        "}\n",
        "class Svc {\n",
        "    /**\n",
        "     * @param list<Widget>|Widget $w\n",
        "     */\n",
        "    public function show(array|Widget $w): void {\n",
        "        if (is_array($w)) {\n",
        "            return;\n",
        "        }\n",
        "        $w->\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 12, 14).await;
    let methods = method_names(&items);

    assert!(
        methods.contains(&"render"),
        "After is_array() guard clause, variable should be Widget; got: {:?}",
        methods
    );
}

// ── negated is_array guard clause ───────────────────────────────────────

#[tokio::test]
async fn test_negated_is_array_guard_clause_narrows_to_array() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///neg_is_array_guard.php").unwrap();
    // `if (!is_array($x)) { return; }` — after the if, $x IS array.
    let text = concat!(
        "<?php\n",
        "class Order {\n",
        "    public function total(): float { return 0.0; }\n",
        "}\n",
        "class Svc {\n",
        "    /**\n",
        "     * @param list<Order>|Order $orders\n",
        "     */\n",
        "    public function batch(array|Order $orders): void {\n",
        "        if (!is_array($orders)) {\n",
        "            return;\n",
        "        }\n",
        "        foreach ($orders as $o) {\n",
        "            $o->\n",
        "        }\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 13, 18).await;
    let methods = method_names(&items);

    assert!(
        methods.contains(&"total"),
        "After !is_array() guard clause, foreach should yield Order; got: {:?}",
        methods
    );
}

// ── is_array then-body with nullable docblock ───────────────────────────

#[tokio::test]
async fn test_is_array_strips_null_and_class_keeps_generic_list() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///is_array_nullable.php").unwrap();
    // `@param null|list<Task>|Task $t` with native `null|array|Task`.
    // After `is_array($t)`, only `list<Task>` survives.
    let text = concat!(
        "<?php\n",
        "class Task {\n",
        "    public function run(): void {}\n",
        "}\n",
        "class Runner {\n",
        "    /**\n",
        "     * @param null|list<Task>|Task $tasks\n",
        "     */\n",
        "    public function execute(null|array|Task $tasks): void {\n",
        "        if (is_array($tasks)) {\n",
        "            foreach ($tasks as $task) {\n",
        "                $task->\n",
        "            }\n",
        "        }\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 11, 23).await;
    let methods = method_names(&items);

    assert!(
        methods.contains(&"run"),
        "After is_array() on null|list<Task>|Task, foreach element should be Task; got: {:?}",
        methods
    );
}

// ── is_object narrows union to class member ─────────────────────────────

#[tokio::test]
async fn test_is_object_narrows_to_class_in_union() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///is_object_narrowing.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Entity {\n",
        "    public function getId(): int { return 0; }\n",
        "}\n",
        "class Svc {\n",
        "    /**\n",
        "     * @param Entity|string $input\n",
        "     */\n",
        "    public function identify(Entity|string $input): void {\n",
        "        if (is_object($input)) {\n",
        "            $input->\n",
        "        }\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 10, 21).await;
    let methods = method_names(&items);

    assert!(
        methods.contains(&"getId"),
        "After is_object(), variable should be narrowed to Entity; got: {:?}",
        methods
    );
}

// ── is_array in elseif ──────────────────────────────────────────────────

#[tokio::test]
async fn test_is_array_in_elseif_narrows_correctly() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///is_array_elseif.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Payload {\n",
        "    public function decode(): string { return ''; }\n",
        "}\n",
        "class Svc {\n",
        "    /**\n",
        "     * @param null|list<Payload>|Payload $p\n",
        "     */\n",
        "    public function handle(null|array|Payload $p): void {\n",
        "        if ($p === null) {\n",
        "            return;\n",
        "        } elseif (is_array($p)) {\n",
        "            foreach ($p as $item) {\n",
        "                $item->\n",
        "            }\n",
        "        }\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 13, 23).await;
    let methods = method_names(&items);

    assert!(
        methods.contains(&"decode"),
        "In elseif(is_array()) body, foreach element should be Payload; got: {:?}",
        methods
    );
}

// ── Simple is_array narrowing without foreach ───────────────────────────
// Verifies the narrowing itself works before testing the foreach pipeline.

#[tokio::test]
async fn test_is_array_direct_access_after_narrowing() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///is_array_direct.php").unwrap();
    // After `is_array($data)`, the variable should NOT offer class members
    // because the type is narrowed to array-only.  But an `Item` in the
    // else branch should offer class members.
    //
    // This test validates narrowing works by checking the else branch
    // (inverse) where `$data` should be `Item` with the `getName` method.
    let text = concat!(
        "<?php\n",
        "class Item {\n",
        "    public function getName(): string { return ''; }\n",
        "}\n",
        "class Svc {\n",
        "    /**\n",
        "     * @param list<Item>|Item $data\n",
        "     */\n",
        "    public function process(array|Item $data): void {\n",
        "        if (is_array($data)) {\n",
        "            // $data is array here\n",
        "        } else {\n",
        "            $data->\n",
        "        }\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 12, 20).await;
    let methods = method_names(&items);

    assert!(
        methods.contains(&"getName"),
        "In else branch of is_array(), $data should be Item with getName; got: {:?}",
        methods
    );
}

// ── Simpler foreach test: docblock-only type, no native union ───────────

#[tokio::test]
async fn test_is_array_foreach_docblock_only() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///is_array_docblock_only.php").unwrap();
    // The parameter has native `array` hint with docblock `list<Request>`.
    // No union involved — just verifying that after is_array(), the docblock
    // element type is preserved through foreach.
    let text = concat!(
        "<?php\n",
        "class Request {\n",
        "    public function jsonSerialize(): array { return []; }\n",
        "}\n",
        "class Svc {\n",
        "    /**\n",
        "     * @param list<Request> $requests\n",
        "     */\n",
        "    public function handle(array $requests): void {\n",
        "        foreach ($requests as $r) {\n",
        "            $r->\n",
        "        }\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 10, 18).await;
    let methods = method_names(&items);

    assert!(
        methods.contains(&"jsonSerialize"),
        "Foreach over list<Request> param should yield Request; got: {:?}",
        methods
    );
}

// ── !is_array guard with reassignment, no inline @var ───────────────────
// Same pattern but without the `/** @var array<User> $recipient */` hint.
// The foreach pipeline should still resolve `$user` to `User` because
// `$recipient` was reassigned to `[$recipient]` where `$recipient` was
// narrowed to `User` by the `!is_array` guard.

#[tokio::test]
async fn test_not_is_array_reassignment_no_var_docblock() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///not_is_array_no_var.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class User {\n",
        "    public string $email = '';\n",
        "    public function getName(): string { return ''; }\n",
        "}\n",
        "class Mailer {\n",
        "    /**\n",
        "     * @param User|array<User> $recipient\n",
        "     */\n",
        "    public function send(User|array $recipient): void {\n",
        "        if (!\\is_array($recipient)) {\n",
        "            $recipient = [$recipient];\n",
        "        }\n",
        "        foreach ($recipient as $user) {\n",
        "            $user->\n",
        "        }\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 14, 20).await;
    let methods = method_names(&items);

    assert!(
        methods.contains(&"getName"),
        "After !is_array() guard with reassignment (no @var), foreach element should be User; got: {:?}",
        methods
    );
}

// ── Inline @var override on reassigned variable in foreach ───────────────
// Simpler variant: no is_array guard, just an inline `@var` on the
// reassigned variable.  Verifies the foreach pipeline picks up the
// inline `@var` annotation on the iterated variable.

#[tokio::test]
async fn test_inline_var_override_on_reassigned_variable_foreach() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///inline_var_reassign.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class User {\n",
        "    public string $email = '';\n",
        "    public function getName(): string { return ''; }\n",
        "}\n",
        "class Mailer {\n",
        "    public function send(User|array $recipient): void {\n",
        "        /** @var array<User> $recipient */\n",
        "        $recipient = [$recipient];\n",
        "        foreach ($recipient as $user) {\n",
        "            $user->\n",
        "        }\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 10, 20).await;
    let methods = method_names(&items);

    assert!(
        methods.contains(&"getName"),
        "After inline @var override, foreach element should be User; got: {:?}",
        methods
    );
}

// ── !is_array guard with reassignment to array ──────────────────────────
// Pattern: `if (!is_array($x)) { $x = [$x]; }` — after the if block,
// $x is always `array<User>`, so foreach should yield `User`.

#[tokio::test]
async fn test_not_is_array_reassignment_to_array_foreach() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///not_is_array_reassign.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class User {\n",
        "    public string $email = '';\n",
        "    public function getName(): string { return ''; }\n",
        "}\n",
        "class Mailer {\n",
        "    /**\n",
        "     * @param User|array<User> $recipient\n",
        "     */\n",
        "    public function send(User|array $recipient): void {\n",
        "        if (!\\is_array($recipient)) {\n",
        "            /** @var array<User> $recipient */\n",
        "            $recipient = [$recipient];\n",
        "        }\n",
        "        foreach ($recipient as $user) {\n",
        "            $user->\n",
        "        }\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 15, 20).await;
    let methods = method_names(&items);

    assert!(
        methods.contains(&"getName"),
        "After !is_array() guard with reassignment, foreach element should be User; got: {:?}",
        methods
    );
}

// ── instanceof narrowing on array access expressions ────────────────────

#[tokio::test]
async fn test_instanceof_narrows_array_access_expression() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///instanceof_array_access.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Page {\n",
        "    public function getId(): int { return 1; }\n",
        "}\n",
        "class Table {\n",
        "    /** @return array<int, array<string, mixed>> */\n",
        "    public function getRows(): array { return []; }\n",
        "}\n",
        "function test(Table $table): void {\n",
        "    foreach ($table->getRows() as $row) {\n",
        "        if ($row['page'] instanceof Page) {\n",
        "            $row['page']->\n",
        "        }\n",
        "    }\n",
        "}\n",
    );
    let items = complete_at(&backend, &uri, text, 11, 28).await;
    let methods = method_names(&items);
    assert!(
        methods.contains(&"getId"),
        "After instanceof narrowing on array access, should see Page methods; got: {:?}",
        methods
    );
}

// ── Reassignment inside a guard branch with a partially-unresolved
//    initial type ────────────────────────────────────────────────────────

#[tokio::test]
async fn test_guard_reassignment_keeps_narrowed_fallthrough_enum() {
    // `$type` starts as `Country|mixed` (the `mixed` from a call whose
    // return type is unknown).  The guard reassigns only the null /
    // non-Country path; after it, `$type` must still be `Country` so its
    // members complete.  Before the fix the unresolved `mixed` component
    // caused the narrowed fall-through (`Country`) to be dropped, leaving
    // no type at all.
    let backend = create_test_backend();
    let uri = Url::parse("file:///guard_reassign_enum.php").unwrap();
    let text = concat!(
        "<?php\n",
        "enum Country {\n",
        "    case ADMIN;\n",
        "    public function getFlagImageName(): string { return 'x'; }\n",
        "}\n",
        "class M {\n",
        "    public function grab(): mixed { return null; }\n",
        "}\n",
        "class C {\n",
        "    public function m(bool $b, M $m): void {\n",
        "        $type = $b ? Country::ADMIN : $m->grab();\n",
        "        if (!$type || !$type instanceof Country) {\n",
        "            $type = Country::ADMIN;\n",
        "        }\n",
        "        $type->\n",
        "    }\n",
        "}\n",
    );
    let items = complete_at(&backend, &uri, text, 14, 15).await;
    let methods = method_names(&items);
    assert!(
        methods.contains(&"getFlagImageName"),
        "After the guard reassignment, `$type` should be `Country`; got: {methods:?}",
    );
}

#[tokio::test]
async fn test_guard_reassignment_keeps_narrowed_fallthrough_class() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///guard_reassign_class.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Country {\n",
        "    public function getFlagImageName(): string { return 'x'; }\n",
        "}\n",
        "class M {\n",
        "    public function grab(): mixed { return null; }\n",
        "}\n",
        "class C {\n",
        "    public function m(bool $b, M $m): void {\n",
        "        $type = $b ? new Country() : $m->grab();\n",
        "        if (!$type || !$type instanceof Country) {\n",
        "            $type = new Country();\n",
        "        }\n",
        "        $type->\n",
        "    }\n",
        "}\n",
    );
    let items = complete_at(&backend, &uri, text, 13, 15).await;
    let methods = method_names(&items);
    assert!(
        methods.contains(&"getFlagImageName"),
        "After the guard reassignment, `$type` should be `Country`; got: {methods:?}",
    );
}

// ── array<T>|false union keeps element type after narrowing ──────────────

#[tokio::test]
async fn test_array_false_union_keeps_element_after_is_array_guard() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///array_false_is_array.php").unwrap();
    // A native `array|false` return refined by a `array<int, self>|false`
    // docblock. After `if (!is_array(...)) return;` the element type must
    // survive so the foreach value resolves to the class.
    let text = concat!(
        "<?php\n",
        "class Col {\n",
        "    public function value(): int { return 0; }\n",
        "    /** @return array<int, self>|false */\n",
        "    public static function getColumns(bool $x): array|false { return false; }\n",
        "}\n",
        "class Svc {\n",
        "    public function run(bool $x): void {\n",
        "        $columns = Col::getColumns($x);\n",
        "        if (!is_array($columns)) {\n",
        "            return;\n",
        "        }\n",
        "        foreach ($columns as $column) {\n",
        "            $column->\n",
        "        }\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 13, 21).await;
    let methods = method_names(&items);

    assert!(
        methods.contains(&"value"),
        "After !is_array() guard on array<int, self>|false, foreach element should be Col; got: {:?}",
        methods
    );
}

#[tokio::test]
async fn test_array_false_union_keeps_element_after_false_check() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///array_false_eqeq.php").unwrap();
    // Same as above, but guarding with `=== false` instead of `is_array`.
    let text = concat!(
        "<?php\n",
        "class Col {\n",
        "    public function value(): int { return 0; }\n",
        "    /** @return array<int, self>|false */\n",
        "    public static function getColumns(bool $x): array|false { return false; }\n",
        "}\n",
        "class Svc {\n",
        "    public function run(bool $x): void {\n",
        "        $columns = Col::getColumns($x);\n",
        "        if ($columns === false) {\n",
        "            return;\n",
        "        }\n",
        "        foreach ($columns as $column) {\n",
        "            $column->\n",
        "        }\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 13, 21).await;
    let methods = method_names(&items);

    assert!(
        methods.contains(&"value"),
        "After `=== false` guard on array<int, self>|false, foreach element should be Col; got: {:?}",
        methods
    );
}

#[tokio::test]
async fn test_instanceof_guard_narrows_repeated_method_call() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///repeated_call_completion.php").unwrap();
    // The shape from the Symfony kernel: the guard checks one call and
    // the body makes the same call again, so the second one has to know
    // what the check proved.
    let text = concat!(
        "<?php\n",
        "interface Kernel {\n",
        "    public function handle(): int;\n",
        "}\n",
        "interface Terminable extends Kernel {\n",
        "    public function terminate(): void;\n",
        "}\n",
        "class App {\n",
        "    public function getKernel(): Kernel { return $this->kernel; }\n",
        "    public function run(): void {\n",
        "        if ($this->getKernel() instanceof Terminable) {\n",
        "            $this->getKernel()->\n",
        "        }\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 11, 33).await;
    let methods = method_names(&items);

    assert!(
        methods.contains(&"terminate"),
        "Inside the `instanceof Terminable` guard the repeated \
         `$this->getKernel()` call should offer `terminate`; got: {:?}",
        methods
    );
}

// ── is_iterable narrowing ───────────────────────────────────────────────

/// `is_iterable()` keeps the members `foreach` can walk: an array in any
/// spelling, and an object whose interfaces reach `Traversable`.  An
/// ordinary object is dropped along with the scalars.
#[tokio::test]
async fn is_iterable_keeps_only_the_members_that_can_be_iterated() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///is_iterable.php").unwrap();
    let text = concat!(
        "<?php\n",
        "interface Traversable {}\n",
        "interface IteratorAggregate extends Traversable {}\n",
        "class Bag implements IteratorAggregate {\n",
        "    public function count(): int { return 0; }\n",
        "}\n",
        "class Plain {\n",
        "    public function plainOnly(): void {}\n",
        "}\n",
        "class Svc {\n",
        "    public function run(Bag|Plain|string $value): void {\n",
        "        if (is_iterable($value)) {\n",
        "            $value->\n",
        "        }\n",
        "    }\n",
        "}\n",
    );

    // Line 12: `            $value->` — cursor after `->`
    let items = complete_at(&backend, &uri, text, 12, 20).await;
    let names = method_names(&items);
    assert!(
        names.contains(&"count"),
        "Bag reaches Traversable, so is_iterable keeps it, got: {:?}",
        names
    );
    assert!(
        !names.contains(&"plainOnly"),
        "Plain implements nothing foreach can walk, got: {:?}",
        names
    );
}

/// The else branch is the inverse proof: what `foreach` cannot walk is
/// exactly what is left.
#[tokio::test]
async fn the_else_branch_of_is_iterable_keeps_the_non_iterable_members() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///is_iterable_else.php").unwrap();
    let text = concat!(
        "<?php\n",
        "interface Traversable {}\n",
        "interface IteratorAggregate extends Traversable {}\n",
        "class Bag implements IteratorAggregate {\n",
        "    public function count(): int { return 0; }\n",
        "}\n",
        "class Plain {\n",
        "    public function plainOnly(): void {}\n",
        "}\n",
        "class Svc {\n",
        "    public function run(Bag|Plain $value): void {\n",
        "        if (is_iterable($value)) {\n",
        "            return;\n",
        "        }\n",
        "        $value->\n",
        "    }\n",
        "}\n",
    );

    // Line 14: `        $value->` — cursor after `->`
    let items = complete_at(&backend, &uri, text, 14, 16).await;
    let names = method_names(&items);
    assert!(
        names.contains(&"plainOnly"),
        "Plain is what survives a failed is_iterable, got: {:?}",
        names
    );
    assert!(
        !names.contains(&"count"),
        "Bag passed the guard, so it cannot be here, got: {:?}",
        names
    );
}

/// PHPUnit's `assertIsIterable()` carries `@phpstan-assert iterable`,
/// which names no class, so it narrows through the guard channel rather
/// than the instanceof one.
#[tokio::test]
async fn an_iterable_assertion_tag_narrows_its_argument() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///assert_iterable.php").unwrap();
    let text = concat!(
        "<?php\n",
        "interface Traversable {}\n",
        "interface IteratorAggregate extends Traversable {}\n",
        "class Bag implements IteratorAggregate {\n",
        "    public function count(): int { return 0; }\n",
        "}\n",
        "class Plain {\n",
        "    public function plainOnly(): void {}\n",
        "}\n",
        "class Assert {\n",
        "    /** @phpstan-assert iterable $actual */\n",
        "    public static function assertIsIterable(mixed $actual): void {}\n",
        "}\n",
        "class Svc {\n",
        "    /** @param Bag|Plain $value */\n",
        "    public function run($value): void {\n",
        "        Assert::assertIsIterable($value);\n",
        "        $value->\n",
        "    }\n",
        "}\n",
    );

    // Line 17: `        $value->` — cursor after `->`
    let items = complete_at(&backend, &uri, text, 17, 16).await;
    let names = method_names(&items);
    assert!(
        names.contains(&"count"),
        "the asserted iterable keeps Bag, got: {:?}",
        names
    );
    assert!(
        !names.contains(&"plainOnly"),
        "Plain cannot be iterated, got: {:?}",
        names
    );
}
