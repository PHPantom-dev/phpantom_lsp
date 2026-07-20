use crate::common::{
    create_psr4_workspace, create_test_backend, create_test_backend_with_full_stubs,
};
use tower_lsp::lsp_types::*;

// ─── Helpers ────────────────────────────────────────────────────────────────

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
// Closure with unresolvable param type still resolves $this
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn closure_with_unresolvable_param_still_resolves_this() {
    let backend = create_test_backend();

    // Register the Collection class so `$this->getName()` resolves.
    let collection_uri = "file:///Collection.php";
    let collection_text = r#"<?php
class Collection {
    /** @return string */
    public function getName(): string { return ''; }
}
"#;
    backend.update_ast(collection_uri, collection_text);

    let service_uri = "file:///Service.php";
    let service_text = r#"<?php
class Service {
    /** @return string */
    public function getLabel(): string { return ''; }

    public function run(): void {
        // The callable param type `collection-of<T>` is a PHPStan
        // pseudo-type that is unresolvable.  Previously this caused
        // the entire closure body to be skipped by the forward
        // walker, so $this would fall through to the backward
        // scanner.  Now the forward walker walks the body, seeding
        // $this from the outer scope.
        $this->getLabel();
    }
}
"#;
    let diags = unknown_member_diagnostics_with_scope_cache(&backend, service_uri, service_text);
    // `$this->getLabel()` should NOT be flagged as unknown.
    assert!(
        diags.is_empty(),
        "Expected no unknown_member diagnostics for $this->getLabel(), got: {diags:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Closure with unresolvable param still resolves use-captured variables
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn closure_with_unresolvable_param_still_resolves_use_vars() {
    let backend = create_test_backend();

    let product_uri = "file:///Product.php";
    let product_text = r#"<?php
class Product {
    /** @return string */
    public function getTitle(): string { return ''; }
}
"#;
    backend.update_ast(product_uri, product_text);

    let uri = "file:///test_unresolvable_use.php";
    let text = r#"<?php
class Handler {
    public function handle(): void {
        $product = new Product();
        $fn = function($unknown) use ($product) {
            $product->getTitle();
        };
    }
}
"#;
    let diags = unknown_member_diagnostics_with_scope_cache(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "Expected no unknown_member diagnostics for use-captured $product->getTitle(), got: {diags:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Closure with mix of resolvable and unresolvable params resolves good ones
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn closure_with_mixed_resolvable_and_unresolvable_params() {
    let backend = create_test_backend();

    let builder_uri = "file:///Builder.php";
    let builder_text = r#"<?php
class Builder {
    /** @return static */
    public function where(string $col, mixed $val): static { return $this; }
}
"#;
    backend.update_ast(builder_uri, builder_text);

    let uri = "file:///test_mixed_params.php";
    let text = r#"<?php
class MyService {
    public function run(): void {
        $fn = function(Builder $query) {
            $query->where('id', 1);
        };
    }
}
"#;
    let diags = unknown_member_diagnostics_with_scope_cache(&backend, uri, text);
    // `$query->where(...)` should resolve fine because Builder is a
    // resolvable param — even if other params in the same closure were
    // unresolvable, the good ones should still be seeded.
    assert!(
        diags.is_empty(),
        "Expected no unknown_member diagnostics for $query->where(), got: {diags:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Multi-@var docblock inside closure overrides parameter types
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn multi_var_docblock_inside_closure_overrides_params() {
    let backend = create_test_backend();

    let app_uri = "file:///App.php";
    let app_text = r#"<?php
class App {
    /** @return object */
    public function make(string $class): object { return new \stdClass; }
}
"#;
    backend.update_ast(app_uri, app_text);

    let client_uri = "file:///Client.php";
    let client_text = r#"<?php
class Client {
    /** @return string */
    public function search(): string { return ''; }
}
"#;
    backend.update_ast(client_uri, client_text);

    let uri = "file:///test_multi_var.php";
    let text = r#"<?php
class Service {
    public function register(): void {
        $fn = function ($app, $params) {
            /**
             * @var App                      $app
             * @var array{indexName: string} $params
             */

            /** @var Client $client */
            $client = $app->make(Client::class);
            $client->search();
        };
    }
}
"#;
    let diags = unknown_member_diagnostics_with_scope_cache(&backend, uri, text);
    // Both `$app->make(...)` and `$client->search()` should resolve
    // thanks to the multi-@var block overriding $app and the single
    // @var block overriding $client.
    assert!(
        diags.is_empty(),
        "Expected no unknown_member diagnostics, got: {diags:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Standalone @var block preceding another @var block before an expression
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn preceding_standalone_var_block_applied_to_scope() {
    let backend = create_test_backend();

    let repo_uri = "file:///Repository.php";
    let repo_text = r#"<?php
class Repository {
    /** @return string */
    public function find(): string { return ''; }
}
"#;
    backend.update_ast(repo_uri, repo_text);

    let mapper_uri = "file:///Mapper.php";
    let mapper_text = r#"<?php
class Mapper {
    /** @return string */
    public function map(mixed $data): string { return ''; }
}
"#;
    backend.update_ast(mapper_uri, mapper_text);

    let uri = "file:///test_preceding_var.php";
    let text = r#"<?php
class Handler {
    public function handle(): void {
        /** @var Repository $repo */

        /** @var Mapper $mapper */
        $result = $mapper->map($repo->find());
    }
}
"#;
    let diags = unknown_member_diagnostics_with_scope_cache(&backend, uri, text);
    // `$mapper->map(...)` resolves from the immediate @var block, and
    // `$repo->find()` resolves from the preceding standalone @var block.
    assert!(
        diags.is_empty(),
        "Expected no unknown_member diagnostics, got: {diags:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// No-var @var override must not leak into the RHS of the same assignment
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn no_var_override_does_not_leak_into_rhs() {
    let backend = create_test_backend();

    let data_uri = "file:///TokenData.php";
    let data_text = r#"<?php
class TokenData {
    /** @return array<string, mixed> */
    public function toArray(): array { return []; }
}
"#;
    backend.update_ast(data_uri, data_text);

    let order_uri = "file:///Orders.php";
    let order_text = r#"<?php
class Orders {
    /** @return mixed */
    public function generateToken(array $data): mixed { return null; }
}
"#;
    backend.update_ast(order_uri, order_text);

    let uri = "file:///test_no_var_rhs.php";
    let text = r#"<?php
class Service {
    public function run(): void {
        $data = new TokenData();
        $orders = new Orders();

        /** @var array<string, mixed> */
        $data = $orders->generateToken($data->toArray());
    }
}
"#;
    let diags = unknown_member_diagnostics_with_scope_cache(&backend, uri, text);
    // `$data->toArray()` on the RHS must still see $data as TokenData,
    // not as the overridden `array<string, mixed>`.
    assert!(
        diags.is_empty(),
        "Expected no unknown_member diagnostics for $data->toArray() in RHS, got: {diags:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Pass-by-ref variable seeded by forward walker (parse_str pattern)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn pass_by_ref_parse_str_seeds_variable_in_scope() {
    let backend = create_test_backend();

    let uri = "file:///test_parse_str.php";
    let text = r#"<?php
class Endpoint {
    public string $queryString = '';

    /** @return array<mixed> */
    public function getParameters(): array
    {
        $parameters = [];
        if ($this->queryString) {
            parse_str($this->queryString, $query);
            foreach ($query as $key => $parameter) {
                if (!is_string($key)) continue;
                $parameters[$key] = $parameter;
            }
        }
        return $parameters;
    }
}
"#;
    // The forward walker should seed $query via pass-by-ref from
    // parse_str, so no fallthrough occurs for $query in the foreach.
    let diags = unknown_member_diagnostics_with_scope_cache(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "Expected no unknown_member diagnostics, got: {diags:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Pass-by-ref preg_match in if-condition seeds $matches
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn pass_by_ref_preg_match_in_if_condition_seeds_matches() {
    let backend = create_test_backend();

    let uri = "file:///test_preg_match.php";
    let text = r#"<?php
class Parser {
    public function parse(string $msg): ?int
    {
        if (preg_match('/order line (?<LineId>\d+)/i', $msg, $matches) === 1) {
            return (int)$matches['LineId'];
        }
        return null;
    }
}
"#;
    // preg_match passes $matches by reference — the forward walker
    // should seed it via seed_pass_by_ref_in_condition so it doesn't
    // fall through to the backward scanner.
    let diags = unknown_member_diagnostics_with_scope_cache(&backend, uri, text);
    assert!(
        diags.is_empty(),
        "Expected no unknown_member diagnostics, got: {diags:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Assert narrowing on $this inside a top-level closure propagates to
// assignment RHS resolution (Pest test pattern).
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn assert_instanceof_this_in_top_level_closure_propagates_to_assignment_rhs() {
    let backend = create_test_backend();

    let collection_uri = "file:///Collection.php";
    let collection_text = r#"<?php
class Collection {
    /** @return mixed */
    public function firstOrFail() {
        return null;
    }
}
"#;
    backend.update_ast(collection_uri, collection_text);

    let test_case_uri = "file:///TestCase.php";
    let test_case_text = r#"<?php
class TestCase {
    public function createProductCollection(int $count): Collection {
        return new Collection();
    }
}
"#;
    backend.update_ast(test_case_uri, test_case_text);

    {
        let mut cfg = backend.config();
        cfg.diagnostics.unresolved_member_access = Some(true);
        backend.set_config(cfg);
    }

    // Top-level Pest-style closure: `$this` is unbound and only narrowed
    // via `assert($this instanceof TestCase)`.
    let uri = "file:///PestTest.php";
    let text = r#"<?php
function it(string $name, callable $closure): void {}

it('does a thing', function (): void {
    assert($this instanceof TestCase);
    $products = $this->createProductCollection(5);
    $first = $products->firstOrFail();
});
"#;
    backend.update_ast(uri, text);

    let mut diags = Vec::new();
    backend.collect_slow_diagnostics(uri, text, &mut diags);

    // Filter to unknown_member and unresolved_member_access diagnostics
    let relevant_diags: Vec<_> = diags
        .into_iter()
        .filter(|d| {
            d.code.as_ref().is_some_and(|c| {
                matches!(
                    c,
                    tower_lsp::lsp_types::NumberOrString::String(s)
                        if s == "unresolved_member_access" || s == "unknown_member"
                )
            })
        })
        .collect();

    // The `assert($this instanceof TestCase)` narrows `$this` to `TestCase`,
    // so `$this->createProductCollection(5)` returns `Collection`, and
    // `$products->firstOrFail()` resolves.  No member should be unknown or
    // unresolved.
    assert!(
        relevant_diags.is_empty(),
        "Expected no unknown/unresolved member diagnostics after assert($this instanceof TestCase) in top-level closure, got: {:?}",
        relevant_diags
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Self-referencing reassignment: RHS use resolves against pre-assignment scope
// ═══════════════════════════════════════════════════════════════════════════

/// When a variable is reassigned and the RHS references the same variable
/// after an arrow function / closure literal, the reference must resolve
/// against the pre-assignment type, not the reassigned result type.
///
/// Regression test: `$variables` starts as `array`, gets reassigned to
/// `implode()`'s `string` result.  The `$variables` passed to
/// `array_map()` sits *after* the arrow function in source, and used to
/// pick up the post-assignment `string` type from the scope snapshot the
/// closure walk recorded after the arrow body, producing a false
/// "expects array, got string" diagnostic.
#[test]
fn self_referencing_reassignment_uses_pre_assignment_scope() {
    let backend = create_test_backend_with_full_stubs();
    let uri = "file:///self_ref_reassign.php";
    let text = r#"<?php
function render(array $variables): string {
    $variables = implode(', ', array_map(fn (string $v): string => "\${$v}", $variables));
    return $variables;
}
"#;
    backend.update_ast(uri, text);

    let mut diags = Vec::new();
    backend.collect_slow_diagnostics(uri, text, &mut diags);

    let type_errors: Vec<_> = diags
        .iter()
        .filter(|d| {
            d.code.as_ref().is_some_and(
                |c| matches!(c, NumberOrString::String(s) if s == "type_mismatch_argument"),
            )
        })
        .map(|d| d.message.clone())
        .collect();

    assert!(
        type_errors.is_empty(),
        "Expected no argument type mismatch: the $variables passed to array_map() \
         should resolve to its pre-assignment `array` type, got: {type_errors:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Closure param with a declared union type must not collapse to one arm
// ═══════════════════════════════════════════════════════════════════════════

/// When the subject is a union of differently-parameterized collections,
/// the closure parameter's own declared union type must be preserved
/// rather than collapsing to the first collection's element type.
#[test]
fn closure_param_declared_union_wins_over_inferred_element() {
    let backend = create_test_backend();

    let collection_uri = "file:///Collection.php";
    let collection_text = r#"<?php
/**
 * @template TKey
 * @template TValue
 */
class Collection {
    /**
     * @param callable(TValue): bool $callback
     * @return static
     */
    public function filter(callable $callback): static { return $this; }
}
"#;
    backend.update_ast(collection_uri, collection_text);

    let support_uri = "file:///Support.php";
    let support_text = r#"<?php
class CanApply {}
class ViewModel { public int $viewId = 0; }
"#;
    backend.update_ast(support_uri, support_text);

    let service_uri = "file:///Service.php";
    let service_text = r#"<?php
class Service {
    /** @param Collection<int, CanApply>|Collection<int, ViewModel>|Collection<int, \stdClass> $items */
    public function probe(Collection $items): void
    {
        $items->filter(function (CanApply|ViewModel|\stdClass $item): bool {
            return $item->viewId === 1;
        });
    }
}
"#;
    let diags = unknown_member_diagnostics_with_scope_cache(&backend, service_uri, service_text);
    assert!(
        diags.is_empty(),
        "Expected no unknown_member diagnostics: the declared union param type \
         (CanApply|ViewModel|stdClass) must be preserved, got: {diags:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Nested return-type inference must not pollute the diagnostic scope cache
// ═══════════════════════════════════════════════════════════════════════════

/// Regression test: while building the diagnostic scope cache for one file,
/// resolving an assignment whose right-hand side calls a method with an
/// inferred (body-derived) return type re-enters the forward walker to look
/// up a variable in the callee's body. That nested walk uses a fresh scope
/// and, before the fix, recorded its own scope snapshots into the still
/// active diagnostic scope cache. Because the cache is keyed only by byte
/// offset, the callee's snapshots (from a different file) could land at an
/// offset inside the caller's call chain, dropping the local query-builder
/// variable and producing a false "type could not be resolved" diagnostic
/// on one branch of a method but not an adjacent, identical one.
///
/// Here `Factory::query()` has no declared return type, so its type is
/// inferred from `return $b;`. Resolving `$query = $this->factory->query()`
/// in `Runner::ids()` triggers that inference. The single-line
/// `whereBetween('id', [...])->pluck('id')` branch used to be flagged while
/// the multi-line `whereBetween('created', [...])` branch resolved cleanly.
#[test]
fn nested_return_type_inference_does_not_clobber_outer_scope() {
    // `Factory::query()` has no declared return type, so its type is
    // inferred from `return $b;`.  It is declared in the *same* file as
    // `Builder` (after it) so that the inference walk of `query()`'s body
    // records snapshots at byte offsets that collide with `Runner::ids()`'s
    // call chain in the other file — reproducing the cross-file offset
    // collision that made this a "1 error, not reproduced in isolation"
    // bug in the wild.
    let builder_src = r#"<?php
namespace App;

class Builder {
    public function whereBetween(string $c, array $r): Builder { return $this; }
    public function pluck(string $k): array { return []; }
    public function select(string $c): Builder { return $this; }
}

class Factory {
    public function query() {
        $b = new Builder();
        return $b;
    }
}
"#;
    let runner_src = r#"<?php
namespace App;

class Runner {
    private Factory $factory;

    public function ids(bool $x, bool $y): array {
        $query = $this->factory->query();
        if ($x) {
            return $query->select('id')->pluck('id');
        }
        if ($x && $y) {
            return $query->whereBetween('id', [$this->n('a'), $this->n('b')])->pluck('id');
        }
        if ($x && $y) {
            return $query->whereBetween('created', [
                $this->n('c'),
                $this->n('d'),
            ])->pluck('id');
        }
        return [];
    }

    private function n(string $s): int { return 1; }
}
"#;

    let (backend, dir) = create_psr4_workspace(
        r#"{ "autoload": { "psr-4": { "App\\": "app/" } } }"#,
        &[
            ("app/Builder.php", builder_src),
            ("app/Runner.php", runner_src),
        ],
    );

    {
        let mut cfg = backend.config();
        cfg.diagnostics.unresolved_member_access = Some(true);
        backend.set_config(cfg);
    }

    // Register both files under their real on-disk URIs so that the
    // body-return-type inferrer can read `Factory`'s source via
    // `get_file_content` (which falls back to reading the file path).
    let builder_uri = format!("file://{}/app/Builder.php", dir.path().display());
    let runner_uri = format!("file://{}/app/Runner.php", dir.path().display());
    backend.update_ast(&builder_uri, builder_src);
    backend.update_ast(&runner_uri, runner_src);

    let runner_text = runner_src;
    let mut diags = Vec::new();
    backend.collect_slow_diagnostics(&runner_uri, runner_text, &mut diags);

    let unresolved: Vec<_> = diags
        .iter()
        .filter(|d| {
            d.code.as_ref().is_some_and(|c| {
                matches!(
                    c,
                    NumberOrString::String(s)
                        if s == "unresolved_member_access" || s == "unknown_member"
                )
            })
        })
        .map(|d| d.message.clone())
        .collect();

    assert!(
        unresolved.is_empty(),
        "Expected no unresolved/unknown member diagnostics on the query-builder \
         chain: nested return-type inference must not clobber `$query` in the \
         diagnostic scope cache, got: {unresolved:?}"
    );
}
