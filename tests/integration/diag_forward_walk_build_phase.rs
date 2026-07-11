use crate::common::create_test_backend;
use tower_lsp::lsp_types::*;

/// Run full slow diagnostics and return only `unknown_member` diagnostics.
fn unknown_member_diagnostics_with_scope_cache(
    backend: &phpantom_lsp::Backend,
    uri: &str,
    text: &str,
) -> Vec<Diagnostic> {
    backend.update_ast(uri, text);
    let mut out = Vec::new();
    backend.collect_slow_diagnostics(uri, text, &mut out);
    out.retain(|d| {
        d.code
            .as_ref()
            .is_some_and(|c| matches!(c, NumberOrString::String(s) if s == "unknown_member"))
    });
    out
}

// ═══════════════════════════════════════════════════════════════════════════
// Build-phase scope cache: no stale reads during forward walk
// ═══════════════════════════════════════════════════════════════════════════

/// A destructuring assignment whose RHS references an earlier variable
/// should resolve correctly during the build phase without producing
/// spurious `unknown_member` diagnostics.
#[test]
fn no_unknown_member_for_destructured_variable_referencing_earlier_var() {
    let backend = create_test_backend();
    let uri = "file:///build_phase_destruct.php";
    let text = r#"<?php

class Item {
    public string $name;
    public int $value;
}

class Container {
    /** @return array{item: Item, count: int} */
    public function getData(): array {
        return ['item' => new Item(), 'count' => 1];
    }
}

function testDestructuring(): void {
    $container = new Container();
    $data = $container->getData();
    $item = $data['item'];
    $item->name;
}
"#;
    let diags = unknown_member_diagnostics_with_scope_cache(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "Expected no unknown_member diagnostics, got: {diags:#?}"
    );
}

/// Variable assigned from a method call on an earlier variable should
/// resolve during the build phase — the scope resolver injected into
/// `process_destructuring_assignment` and friends should prevent stale
/// cache reads.
#[test]
fn no_unknown_member_for_chained_variable_assignment() {
    let backend = create_test_backend();
    let uri = "file:///build_phase_chain.php";
    let text = r#"<?php

class Logger {
    public function info(string $msg): void {}
}

class App {
    public function getLogger(): Logger {
        return new Logger();
    }
}

function testChain(): void {
    $app = new App();
    $logger = $app->getLogger();
    $logger->info('hello');
}
"#;
    let diags = unknown_member_diagnostics_with_scope_cache(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "Expected no unknown_member diagnostics for chained assignment, got: {diags:#?}"
    );
}

/// Multiple sequential assignments where each RHS depends on the
/// previous variable — the BUILDING_SCOPES flag should prevent the
/// partially-populated cache from being consulted.
#[test]
fn no_unknown_member_for_sequential_dependent_assignments() {
    let backend = create_test_backend();
    let uri = "file:///build_phase_sequential.php";
    let text = r#"<?php

class Config {
    public string $dsn;
}

class Database {
    public function __construct(public Config $config) {}
    public function getConfig(): Config {
        return $this->config;
    }
}

class Service {
    public function __construct(private Database $db) {}
    public function getDatabase(): Database {
        return $this->db;
    }
}

function testSequential(): void {
    $service = new Service(new Database(new Config()));
    $db = $service->getDatabase();
    $config = $db->getConfig();
    $config->dsn;
}
"#;
    let diags = unknown_member_diagnostics_with_scope_cache(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "Expected no unknown_member diagnostics for sequential dependent assignments, got: {diags:#?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Loop-carried variable assignments (two-pass loop body walk)
// ═══════════════════════════════════════════════════════════════════════════

/// A variable assigned late in a `for` loop body should be visible to
/// earlier statements on iteration ≥ 2 (two-pass walk).
#[test]
fn no_unknown_member_for_loop_carried_variable_in_for() {
    let backend = create_test_backend();
    let uri = "file:///loop_carried_for.php";
    let text = r#"<?php

class Period {
    public function diffInDays(Period $other): int {
        return 0;
    }
}

function testForLoop(): void {
    /** @var list<Period> $periods */
    $periods = [];
    $lastEnd = null;
    for ($i = 0; $i < count($periods); $i++) {
        if ($lastEnd !== null) {
            $lastEnd->diffInDays($periods[$i]);
        }
        $lastEnd = $periods[$i];
    }
}
"#;
    let diags = unknown_member_diagnostics_with_scope_cache(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "Expected no unknown_member diagnostics for loop-carried variable in for loop, got: {diags:#?}"
    );
}

/// A variable assigned late in a `while` loop body should be visible to
/// earlier statements on iteration ≥ 2 (two-pass walk).
#[test]
fn no_unknown_member_for_loop_carried_variable_in_while() {
    let backend = create_test_backend();
    let uri = "file:///loop_carried_while.php";
    let text = r#"<?php

class Period {
    public function diffInDays(Period $other): int {
        return 0;
    }
}

function testWhileLoop(): void {
    /** @var list<Period> $periods */
    $periods = [];
    $lastEnd = null;
    $i = 0;
    while ($i < count($periods)) {
        if ($lastEnd !== null) {
            $lastEnd->diffInDays($periods[$i]);
        }
        $lastEnd = $periods[$i];
        $i++;
    }
}
"#;
    let diags = unknown_member_diagnostics_with_scope_cache(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "Expected no unknown_member diagnostics for loop-carried variable in while loop, got: {diags:#?}"
    );
}

/// A variable assigned late in a `do-while` loop body should be visible
/// to earlier statements on iteration ≥ 2 (two-pass walk).
#[test]
fn no_unknown_member_for_loop_carried_variable_in_do_while() {
    let backend = create_test_backend();
    let uri = "file:///loop_carried_do_while.php";
    let text = r#"<?php

class Period {
    public function diffInDays(Period $other): int {
        return 0;
    }
}

function testDoWhileLoop(): void {
    /** @var list<Period> $periods */
    $periods = [];
    $lastEnd = null;
    $i = 0;
    do {
        if ($lastEnd !== null) {
            $lastEnd->diffInDays($periods[$i]);
        }
        $lastEnd = $periods[$i];
        $i++;
    } while ($i < count($periods));
}
"#;
    let diags = unknown_member_diagnostics_with_scope_cache(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "Expected no unknown_member diagnostics for loop-carried variable in do-while loop, got: {diags:#?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Condition assignment inside binary comparison
// ═══════════════════════════════════════════════════════════════════════════

/// A variable assigned inside a condition comparison like
/// `if (($x = expr()) !== null)` should be visible in the then-body.
#[test]
fn no_unknown_member_for_condition_assignment_in_binary_comparison() {
    let backend = create_test_backend();
    let uri = "file:///condition_assign_binary.php";
    let text = r#"<?php

class Connection {
    public function query(string $sql): string {
        return '';
    }
}

class Factory {
    public function create(): ?Connection {
        return new Connection();
    }
}

function testConditionAssignment(): void {
    $factory = new Factory();
    if (($conn = $factory->create()) !== null) {
        $conn->query('SELECT 1');
    }
}
"#;
    let diags = unknown_member_diagnostics_with_scope_cache(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "Expected no unknown_member diagnostics for condition assignment in binary comparison, got: {diags:#?}"
    );
}

/// Same pattern but with null on the left: `if (null !== ($x = expr()))`.
#[test]
fn no_unknown_member_for_condition_assignment_null_lhs() {
    let backend = create_test_backend();
    let uri = "file:///condition_assign_null_lhs.php";
    let text = r#"<?php

class Connection {
    public function query(string $sql): string {
        return '';
    }
}

class Factory {
    public function create(): ?Connection {
        return new Connection();
    }
}

function testConditionAssignmentNullLhs(): void {
    $factory = new Factory();
    if (null !== ($conn = $factory->create())) {
        $conn->query('SELECT 1');
    }
}
"#;
    let diags = unknown_member_diagnostics_with_scope_cache(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "Expected no unknown_member diagnostics for condition assignment with null on LHS, got: {diags:#?}"
    );
}

/// `while (($line = fgets($fp)) !== false)` — assignment in while condition
/// inside a binary comparison.
#[test]
fn no_unknown_member_for_while_condition_assignment_in_binary() {
    let backend = create_test_backend();
    let uri = "file:///while_condition_assign.php";
    let text = r#"<?php

class Row {
    public string $name;
}

class Cursor {
    public function fetch(): ?Row {
        return new Row();
    }
}

function testWhileConditionAssign(): void {
    $cursor = new Cursor();
    while (($row = $cursor->fetch()) !== null) {
        $row->name;
    }
}
"#;
    let diags = unknown_member_diagnostics_with_scope_cache(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "Expected no unknown_member diagnostics for while condition assignment in binary comparison, got: {diags:#?}"
    );
}

/// Bare negated assignment guard: `if (!$item = expr()) { return; }`
/// PHP parses this as `!($item = expr())`.  The assignment must register
/// as a def site so `$item` is resolved in the code that follows.
#[test]
fn no_unknown_member_for_negated_condition_assignment() {
    let backend = create_test_backend();
    let uri = "file:///negated_condition_assign.php";
    let text = r#"<?php

class CartItem {
    public function getQty(): int {
        return 0;
    }
}

class CartService {
    public function find(int $id): ?CartItem {
        return new CartItem();
    }

    public function run(int $productId): void {
        if (!$item = $this->find($productId)) {
            return;
        }
        $item->getQty();
    }
}
"#;
    let diags = unknown_member_diagnostics_with_scope_cache(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "Expected no unknown_member diagnostics for negated condition assignment, got: {diags:#?}"
    );
}

/// Assignment wrapped in a call argument:
/// `while (is_object($token = $tokenizer->next()))`.  The assignment must
/// register as a def site so `$token` is resolved inside the loop body.
#[test]
fn no_unknown_member_for_call_wrapped_condition_assignment() {
    let backend = create_test_backend();
    let uri = "file:///call_wrapped_condition_assign.php";
    let text = r#"<?php

class Token {
    public string $type;
}

class Tokenizer {
    public function next(): ?Token {
        return new Token();
    }
}

function testCallWrappedAssign(): void {
    $tokenizer = new Tokenizer();
    $actual = [];
    while (is_object($token = $tokenizer->next())) {
        $actual[] = $token->type;
    }
}
"#;
    let diags = unknown_member_diagnostics_with_scope_cache(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "Expected no unknown_member diagnostics for call-wrapped condition assignment, got: {diags:#?}"
    );
}
