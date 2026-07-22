use crate::common::create_test_backend;
use tower_lsp::lsp_types::*;

/// Open a file, run full slow diagnostics (which activates the diagnostic
/// scope cache and the forward walker), then filter to unknown_member
/// diagnostics only.
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
// Property chain accesses must not produce false-positive unknown_member
// diagnostics.  The scope cache is keyed by bare `$variable` names and
// cannot serve property chains like `$this->query->joins`.  The guard in
// resolve_target_classes_expr_inner must skip the resolve_variable_types
// call for such chains so the other resolution strategies (property type
// hints, docblocks, etc.) handle them correctly.
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_false_positive_on_property_chain_method_call() {
    let backend = create_test_backend();
    let uri = "file:///test/chain.php";
    let text = r#"<?php

class Inner {
    public function doStuff(): string {
        return 'ok';
    }
}

class Outer {
    /** @var Inner */
    public Inner $inner;

    public function __construct() {
        $this->inner = new Inner();
    }

    public function run(): void {
        $this->inner->doStuff();
    }
}
"#;
    let diags = unknown_member_diagnostics_with_scope_cache(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "Expected no unknown_member diagnostics for property chain method call, got: {diags:#?}"
    );
}

#[test]
fn no_false_positive_on_deep_property_chain() {
    let backend = create_test_backend();
    let uri = "file:///test/deep_chain.php";
    let text = r#"<?php

class Level2 {
    public function leaf(): int {
        return 42;
    }
}

class Level1 {
    /** @var Level2 */
    public Level2 $level2;
}

class Root {
    /** @var Level1 */
    public Level1 $level1;

    public function go(): void {
        $this->level1->level2->leaf();
    }
}
"#;
    let diags = unknown_member_diagnostics_with_scope_cache(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "Expected no unknown_member diagnostics for deep property chain, got: {diags:#?}"
    );
}

#[test]
fn no_false_positive_on_variable_property_chain() {
    let backend = create_test_backend();
    let uri = "file:///test/var_chain.php";
    let text = r#"<?php

class Query {
    /** @var array */
    public array $joins = [];

    public function getJoins(): array {
        return $this->joins;
    }
}

class Builder {
    /** @var Query */
    public Query $query;

    public function __construct() {
        $this->query = new Query();
    }

    public function build(): void {
        $this->query->getJoins();
    }
}
"#;
    let diags = unknown_member_diagnostics_with_scope_cache(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "Expected no unknown_member diagnostics for \
         $this->query->getJoins() chain, got: {diags:#?}"
    );
}

#[test]
fn still_flags_unknown_member_on_property_chain() {
    let backend = create_test_backend();
    let uri = "file:///test/chain_unknown.php";
    let text = r#"<?php

class Service {
    public function valid(): string {
        return 'ok';
    }
}

class Controller {
    /** @var Service */
    public Service $service;

    public function handle(): void {
        $this->service->nonExistentMethod();
    }
}
"#;
    let diags = unknown_member_diagnostics_with_scope_cache(&backend, uri, text);
    assert!(
        !diags.is_empty(),
        "Expected an unknown_member diagnostic for nonExistentMethod() on property chain"
    );
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("nonExistentMethod")),
        "Diagnostic should mention nonExistentMethod, got: {diags:#?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Deeply nested calls that pass closures/arrow functions to array_map /
// array_filter used to recurse without bound: resolving an inline call
// argument re-walks the whole file at the same cursor, so a nested
// `array_map(...)` inside `array_filter(...)` re-reached the same call and
// re-requested its own raw type until the stack overflowed.  The forward
// walker must complete instead of aborting the process.
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn nested_array_map_filter_closures_do_not_overflow() {
    let backend = create_test_backend();
    let uri = "file:///test/nested_closures.php";
    let text = r#"<?php

$envVars = array_filter(
    array_map(
        fn($arg) => array_slice(explode("=", trim($arg, "\"'")), 1, 2),
        array_filter(
            $argv,
            fn($arg) => str_starts_with(trim($arg, "\"'"), "--env="),
        ),
    ),
    fn($keyAndValue) => is_array($keyAndValue) && count($keyAndValue) === 2,
);
"#;
    // Running slow diagnostics exercises the forward walker + inline
    // argument resolution.  A regression here aborts the whole test
    // process with a stack overflow, so simply reaching the assertion
    // proves the cycle is broken.
    let _diags = unknown_member_diagnostics_with_scope_cache(&backend, uri, text);
}
