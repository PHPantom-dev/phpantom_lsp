# Test Porting Plan

This document tracks the plan for porting tests from reference projects
(Mago, Larastan, PHPStan, Psalm) into PHPantom's integration test suite.
The goal is to use battle-tested test cases from mature PHP analysis tools
to find gaps and mistakes in our type engine, diagnostics, and LSP
features.

## Principles

1. **Mago first, then Larastan, then PHPStan.** Mago tests are
   standalone PHP, closest to our architecture, and easiest to convert.
   Larastan tests require more judgment. PHPStan tests are gospel but
   need a fixture runner.

2. **Trust levels differ by source.** Mago tests are generally
   trustworthy unless surprising. Larastan tests need case-by-case
   evaluation (they do runtime analysis that does not always align with
   sound static analysis). PHPStan and Psalm tests are the gold
   standard — Psalm is equally rigorous, with clean assertion-based
   tests that map directly to type resolution accuracy.

3. **Skip or replace, don't duplicate.** When a ported test covers the
   same ground as an existing PHPantom test, either skip the port or
   replace the existing test if the ported version is better.

4. **Tests must be adapted, not copied verbatim.** Reference tests use
   their own harnesses (`@mago-expect`, PHPStan's `assertType()`). Each
   must be translated into PHPantom's `create_test_backend()` pattern
   or a new fixture runner format.

5. **File per concern.** Ported tests go into existing test files when
   the feature area matches (e.g. narrowing tests into
   `completion_guard_clauses.rs` or `completion_variables.rs`). When a
   batch of ported tests introduces a new concern, create a new file.

---

## PHPStan Type Scope Policy

PHPantom is an LSP, not a linter. We proxy PHPStan for linter-grade
diagnostics. When evaluating PHPStan tests for porting, apply these
rules:

| PHPStan type | PHPantom treatment | Port tests? |
|---|---|---|
| `non-empty-string`, `non-falsy-string`, `lowercase-string`, `uppercase-string`, `numeric-string` | Treat as `string`. Must not trigger unknown-type diagnostics. | No |
| `non-empty-array`, `non-empty-list` | Treat as `array`/`list`. | No |
| `int<min, max>`, `positive-int`, `negative-int` | Treat as `int`. | No |
| Literal strings (`'foo'`), literal ints (`1`), literal floats (`1.0`) | Full support. Port the upstream expectation verbatim. | **Yes** |
| Literal bools (`true`, `false`) | Treat as `bool` (`LiteralValue` has no bool variant). | No |
| `Enum::CASE` (literal enum case) | Treat as instance of the enum class. | **Yes** (needs implementation) |
| Array shapes (`array{key: type}`) | Full support (drives array key completion). | **Yes** |
| `preg_match` group shapes | Infer `$matches` shape from regex (array key completion). | **Yes** (needs implementation: regex pattern parsing) |
| Closure/callable types (`Closure(T): U`) | Relevant for completion on closure return values. | **Yes** (needs implementation: closure type inference) |

**Rule of thumb:** If a test's assertions are exclusively about
narrowing `string` to `non-empty-string` or `int` to `positive-int`,
skip it. If it tests something that affects completion, hover, or
go-to-definition (shapes, enum cases, closure returns), port it.

**Scalar literals were previously out of scope** and expectations were
rewritten to the base type on the way in (`'foo'` → `string`). The type
engine now preserves scalar literals through expression resolution, so
that rewrite no longer matches either the upstream corpus or PHPantom.
Port literal expectations verbatim instead. Two categories of
already-ported downgrade remain, because the engine still widens them:
literal bools (a deliberate scope decision, above) and class constants
(`self::INTEGER_CONSTANT` resolves to `int` where PHPStan says `1` — see
T34 in [type-inference.md](type-inference.md)).

---

## PHPStan assertType Fixture Runner

The runner lives in `tests/assert_type_runner.rs` and is registered as
a `datatest_stable` harness in `Cargo.toml`. It picks up `.php` files
from `tests/phpstan_nsrt/` and `tests/psalm_assertions/`.

**How it works:**

1. Parses all `assertType('expected', expr)` calls via text scanning.
2. Transforms each call into `$__phpantom_assert_N = expr;` so the
   expression result is assigned to a hoverable variable.
3. Opens the transformed source in a test backend.
4. Hovers on each `$__phpantom_assert_N` to resolve the type.
5. Normalizes both sides (`?T` → `T|null`, FQN shortening, union
   member sorting, literal quote style, `self` acceptance) before
   comparing.

Lines with `// SKIP` are skipped. `assertNativeType` calls are ignored.
Commented-out lines (starting with `//`) are ignored.

The backend is built with the full embedded stubs, at the default PHP
version (8.5), so builtin functions and classes resolve the way they do for
a user. (It used to load a handful of hand-written class stubs and no
function stubs at all, which made every builtin call resolve to nothing and
let `mixed` expectations pass for the wrong reason.) A fixture whose
upstream header says `// lint < 8.x` is out of scope for that reason.

**Matching is exact.** Normalization covers cosmetic spellings only
(positional shape keys, PHPStan's template-scope suffix, `callable(): mixed`,
`mixed[]`, redundant parentheses, `mixed` absorbing a union, and the like),
so a scalar literal never satisfies an expectation written as its base type.
A bare `self` from PHPantom no longer matches any expected class.
When PHPantom is *more* precise than the source corpus, record the
precise type and add a comment saying so (the ported Psalm files use
"PHPantom is more precise than Psalm here"). When it is *less* precise,
keep the upstream expectation and mark the line `// SKIP`, so the gap
shows up as a known gap rather than being absorbed silently.

PHPantom keeps a string literal's source spelling, so `"hello"` and
`'hello'` are both possible actual types; the runner canonicalizes quote
style, which lets upstream expectations (always single-quoted) port
unchanged.

**Psalm extraction:** `scripts/extract_psalm_tests.php` parses Psalm's
PHP test classes and emits standalone `.php` files in
`tests/psalm_assertions/` with `assertType()` calls. Run with `--all`
to regenerate all files.

---

## Completed Phases

| Phase | Source | Result |
|-------|--------|--------|
| 1 | Mago (easy, ~100 files) | **Done.** All portable patterns ported. |
| 2 | Larastan (easy, ~15 files) | **Done.** All portable patterns ported. |
| 3 | PHPStan (actionable, ~26 files) | **Done.** All bugs filed during porting (B23, B40-B48) fixed. |
| 3.5A | Psalm (clearly relevant) | **Done.** 19 files, 116 running assertions, 5 bugs filed (B1-B5). |
| 3.5B | Psalm (partial) | **Done.** 2 files kept (23 running assertions), 4 deferred to Phase 5. |
| 4B | Trait resolution (Mago, 6 files) | **Done.** 6 tests ported to `completion_traits.rs`, all passing. |
| 5G | Un-SKIP Psalm assertions | **Done.** Targeted SKIP markers removed; all fixture assertions now pass. Four SKIPs added later, tracked in 5G. |

---

## Phase 4: Harder Ports (Second Round)

### 4A. Mago Issue Regressions — DONE (14/22 ported, 8 triaged out)

352 files (333 unique issues). Each file has `@mago-expect` annotations
indicating which Mago diagnostic rule it tests. Unlike PHPStan's
`assertType()` files, these don't directly assert types — they assert
"Mago emits warning X." Porting requires reading each file,
identifying the underlying type resolution pattern, and writing a
PHPantom test for that pattern.

**Bulk skip (~250 files):** Linter-only rules (`redundant-condition`,
`impossible-condition`, `unused-*`, `missing-*`, `incorrect-*-casing`,
`less-specific-*`, `unreachable-*`, `unhandled-thrown-type`,
`experimental-usage`, etc.) plus out-of-scope rules (literal types,
`non-empty-*`, integer ranges). No type resolution value for an LSP.

**Individually triaged (~100 files):** Read each file, identified the
underlying type resolution pattern, checked against existing PHPantom
tests. Results:

**Already covered (skip, 5 patterns):**
- `class-string<T>` dynamic instantiation (627)
- Nullsafe chain resolution (731)
- `static` return through inheritance (880)
- `#[Deprecated]` attribute (1541)
- Array built in loop then accessed (1574)

**Portable patterns with missing/partial coverage (22 patterns):**

| Issue | Pattern | Test area | Coverage |
|-------|---------|-----------|----------|
| 275 | `array_column()` return type inference | `completion_variables` | Blocked (function return type provider, overlaps 5I/5J) |
| 362 | Generic class implementing `ArrayAccess`/`IteratorAggregate` | `completion_generics` | Blocked ([T42](type-inference.md#t42-a-static-factorys-own-return-selft-breaks-every-chained-call-after-it)) |
| 448 | `??` narrows nullable to non-null type | `completion_variables` | **Ported** |
| 523 | Unsealed array shapes with `...` spread syntax | `completion_array_shapes` | **Ported** |
| 557 | `isset()` narrowing on optional array shape keys | `completion_variables` | **Ported** |
| 570 | Compound `isset()` on nested optional shape keys | `completion_array_shapes` | **Ported** |
| 623 | Union of array shapes — key access | `completion_array_shapes` | **Ported** |
| 676 | Dependent template bounds (T bounded by another template S) | — | Skip (diagnostic-only: `template-constraint-violation`, no completion/hover value; same family as 1355) |
| 712 | Generic `ArrayAccess` with concrete type annotation | — | Skip (diagnostic-only: `invalid-array-index`/`invalid-array-access-assignment-value`, no completion/hover value) |
| 964 | `static` return preserving generic substitution from caller | `completion_generics` | **Ported** |
| 1002 | Intersection types with `UnitEnum` resolving `::cases()` | `completion_enums` | **Ported** |
| 1025 | `?->` branch narrowing (null not narrowed in else) | `completion_array_shapes` | **Ported** |
| 1031 | Closure param type inference from `array_filter` signature | `completion_variables` | Blocked (closure param inference, overlaps 5A) |
| 1038 | Switch statement type narrowing | `completion_guard_clauses` | **Ported** |
| 1040 | `@phpstan-import-type` combined with `@template-implements` | `completion_type_aliases` | **Ported** |
| 1045 | `array_walk` callback parameter type inference | `completion_variables` | Blocked (closure param inference, overlaps 5A) |
| 1061 | Enum implementing generic interface with `@implements` | `completion_enums` | **Ported** |
| 1064/1070 | `@psalm-require-extends`/`@require-implements` trait member resolution | `completion_traits` | **Ported** (property-hook member via `$this->`); the `self::`/`static::` half is blocked ([C13](completion.md#c13-selfstatic-inside-a-require-extends-trait-does-not-see-the-required-classs-static-members)) |
| 1093 | Array shape narrowing via `array_key_exists` check | `completion_array_shapes` | **Ported** |
| 1116 | Nested `@psalm-import-type` alias resolution | `completion_type_aliases` | **Ported** |
| 870 | `@type` alias used inside `@extends` | `completion_generics` | Blocked ([T43](type-inference.md#t43-selftypealias-inside-extendss-generic-argument-is-not-resolved)) |
| 830 | `array_filter` with first-class callables | `completion_variables` | Blocked (first-class callables + assert-if-true narrowing, overlaps 5A/5J) |

**Blocked (need new features, moved to Phase 5):**
- 1226: PHP 8.4 property hooks type resolution
- 1355: Template constraint violation diagnostic

**Ported (14 patterns):**
- `completion_array_shapes` — 6 tests (523, 557, 570, 623, 1025, 1093)
- `completion_generics` — 2 tests (964, and the dual-template `self<K, V>`
  static-factory half of 362 that turned out already broken — see T42)
- `completion_variables` — 1 test (448)
- `completion_guard_clauses` — 1 test (1038)
- `completion_enums` — 2 tests (1061, 1002)
- `completion_type_aliases` — 2 tests (1040, 1116)
- `completion_traits` — 1 test (1064/1070, property-hook half)

Porting 1040 and 1116 surfaced a real completion bug (not a missing
feature): array key completion on a `@var`/`@param`-annotated variable
whose type is a *named* `@phpstan-type`/`@psalm-type` alias (rather than
an inline `array{...}` shape) returned nothing at all, and a chain of
aliases referencing each other by name only expanded the first hop.
Both are fixed in `completion/array_shape.rs` (see the CHANGELOG entry)
and the two ported tests exercise the fix directly.

**Remaining (8 patterns), all blocked on something other than test
content:**
- `completion_generics` — 362 ([T42](type-inference.md#t42-a-static-factorys-own-return-selft-breaks-every-chained-call-after-it)), 870 ([T43](type-inference.md#t43-selftypealias-inside-extendss-generic-argument-is-not-resolved))
- `completion_variables` — 275, 1031, 1045, 830 (function return type providers / closure param inference, overlap Phase 5)
- `completion_traits` — the `self::`/`static::` half of 1064/1070 ([C13](completion.md#c13-selfstatic-inside-a-require-extends-trait-does-not-see-the-required-classs-static-members))
- skipped outright, no LSP value: 676, 712

### 4C. Psalm Non-Assertion Patterns — DONE (7/8 ported)

The eight valid-code files have no `$var => 'Type'` assertions, so each
snippet was hand-ported into an `assertType()` fixture in
`tests/psalm_assertions/` stating the type the snippet implies (a getter or
concrete subclass was added where upstream's classes were empty). Cases
whose result is out of scope under the type policy above (`non-empty-*`,
`numeric-string`, a literal bool narrowed out of `bool`) were left out.

| Psalm file | Fixture | Assertions |
|------------|---------|-----------:|
| `Template/TraitTemplateTest.php` | `template_trait_template.php` | 16 |
| `Template/NestedTemplateTest.php` | `template_nested_template.php` | 14 |
| `Template/ClassStringMapTest.php` | `template_class_string_map.php` | 5 |
| `ConstValuesTest.php` | `const_values.php` | 7 (5 SKIP) |
| `KeyOfArrayTest.php` | `key_of_array.php` | 9 (2 SKIP) |
| `VariadicTest.php` | `variadic.php` | 8 |
| `TypeReconciliation/ReconcilerTest.php` | `type_reconciliation_reconciler.php` | 30 (15 SKIP) |
| `ReturnTypeProvider/ArraySliceTest.php` | not ported | — |

`ArraySliceTest` has one case, and its only observable type is
`array_slice()`'s return. The runner now loads the function stubs, so it is
worth retrying alongside 5J.

Porting fixed seven engine bugs (vendor `@param` tags ignored in the body,
variadics typed as lists, nested generic trait `@use` not substituted, a
template named like a class read as that class in return checks, and three
`key-of` gaps) plus a runner bug that corrupted every block after a
multi-line `assertType()`. The rest are `// SKIP` and filed: B343-B347 in
[bugs.md](bugs.md) and the broadened T26 in
[type-inference.md](type-inference.md).

### Phase 4: done

---

## Phase 5: Requires New Features

These tests are portable once the corresponding feature is implemented
in PHPantom. Each group lists the blocking feature.

### 5A. Closures and Callables (blocked: closure type inference)

| PHPStan file (nsrt/) | Assertions | What it tests |
|----------------------|-----------|---------------|
| `closure-return-type.php` | 20 | Closure return type resolution |
| `closure-argument-type.php` | 11 | Closure argument inference |
| `first-class-callables.php` | 16 | `$obj->method(...)` syntax |
| `arrow-function-types.php` | 9 | Arrow function types |

### 5B. Enum Case Types (blocked: `Enum::CASE` as instance type)

| PHPStan file (nsrt/) | Assertions | What it tests |
|----------------------|-----------|---------------|
| `enum-from.php` | 30 | `Enum::from()` / `::tryFrom()` |
| `enum-in-array.php` | 32 | Enum values in `in_array()` |

### 5C. PHPDoc Inheritance (blocked: `static()`/`$this()`, use aliases)

| PHPStan file (nsrt/) | Assertions | What it tests |
|----------------------|-----------|---------------|
| `methodPhpDocs-implicitInheritance.php` | 68 | Implicit PHPDoc inheritance |
| `method-phpDocs-inheritdoc.php` | 68 | `{@inheritdoc}` resolution |

### 5D. Conditional Return Types (blocked: runner limitation)

| PHPStan file (nsrt/) | Assertions | What it tests |
|----------------------|-----------|---------------|
| `conditional-types.php` | 44 | `($x is string ? int : float)` |
| `conditional-types-inference.php` | 17 | Conditional type param inference |

Blocked until runner supports assertions inside anonymous functions, or
tests are restructured into class methods. The feature itself is in
scope and partially working.

### 5E. Array Shape Functions (blocked: partial — shape assertions relevant, rest not)

| PHPStan file (nsrt/) | Assertions | What it tests |
|----------------------|-----------|---------------|
| `array-functions.php` | 177 | Broad array function coverage |
| `array-merge.php` + variants | ~67 | `array_merge()` shapes |

These files mix shape assertions (relevant for array key completion)
with `non-empty-*`/literal assertions (out of scope). Partial porting
possible once someone triages which assertions are shape-related.

### 5F. Regex Match Shapes (blocked: regex pattern parsing)

| PHPStan file (nsrt/) | Assertions | What it tests |
|----------------------|-----------|---------------|
| `preg_match_shapes.php` | 230 | `$matches` array shape from regex |

Direct LSP value: array key completion on `preg_match` results. Needs
regex pattern parsing to infer group names and counts.

### 5H. Callable/Closure Inference (blocked: closure type inference)

| Psalm file | Assertions | What it tests |
|------------|-----------|---------------|
| `CallableTest.php` | 32 | Callable parameter type propagation, closure return inference |

30/32 assertions failed during 3.5B extraction. Almost all require
inferring closure parameter types from callable signatures and
propagating generic type arguments through higher-order functions.
Overlaps with 5A (closures).

**When unblocked:** Run
`php scripts/extract_psalm_tests.php <psalm-checkout>/tests/CallableTest.php`,
curate the output (add SKIP markers to remaining failures), and move the
result into `tests/psalm_assertions/`.

### 5I. Function Return Type Stubs (blocked: function-specific return type providers)

| Psalm file | Assertions | What it tests |
|------------|-----------|---------------|
| `FunctionCallTest.php` | 72 | `explode`, `parse_url`, `hash_init`, `array_column`, etc. |

0/72 assertions passed. These test return types of specific built-in
PHP functions that require per-function return type providers (not
just stub signatures). Many also need conditional return types based
on argument values.

**When unblocked:** Run
`php scripts/extract_psalm_tests.php <psalm-checkout>/tests/FunctionCallTest.php`,
curate the output, and move into `tests/psalm_assertions/`.

### 5J. Array Function Return Types (blocked: function stubs)

| Psalm file | Assertions | What it tests |
|------------|-----------|---------------|
| `ArrayFunctionCallTest.php` | 88 | `array_filter`, `array_keys`, `array_merge`, `array_map`, etc. |

The superlinear hover-scaling bug that used to time out extraction on
the 1095-line generated file is fixed (it is gone from `bugs.md`, and a
fresh run confirms it: the fixture now runs in ~8s instead of timing out
at 126s). That was never the real blocker, though — re-running the
suite today gets 36/168 assertions passing and 132/168 failing, almost
all of them wanting a specific array function's return type provider
(`array_filter`, `array_map`, `array_merge`, etc. losing precision to
`mixed`/`array<array-key, mixed>`) rather than a resolution bug. Overlaps
with 5E.

**When unblocked:** Run
`php scripts/extract_psalm_tests.php <psalm-checkout>/tests/ArrayFunctionCallTest.php`,
curate the output, and move into `tests/psalm_assertions/`.

### 5K. Constant Expression Evaluation (blocked: compile-time constant folding)

| Psalm file | Assertions | What it tests |
|------------|-----------|---------------|
| `ConstantTest.php` | 14 | Class constant types, bitwise const expressions, magic constants |

2/14 passed (trivial cases). Requires evaluating constant expressions
at analysis time (bitwise ops on constants, string concatenation with
`PHP_EOL`, `self::A | self::B`, magic constants like `__CLASS__`).

**When unblocked:** Run
`php scripts/extract_psalm_tests.php <psalm-checkout>/tests/ConstantTest.php`,
curate the output, and move into `tests/psalm_assertions/`.

### 5L. Extract Psalm's literal-typed assertions — DONE

`filterAssertions()` in `scripts/extract_psalm_tests.php` no longer drops
assertions whose expected type is a bare literal int or string (literal
bools are still filtered — PHPantom has no `LiteralValue` bool variant).

Re-running the extraction for all 22 already-ported source files against
the current `references/psalm` checkout, with only the filter changed,
produced **byte-for-byte identical output** to the unrelaxed filter (verified
by diffing both runs against the same checkout state). None of these
files' Psalm test providers assert a bare literal as the whole type;
where PHPantom is more precise than Psalm's own `int`/`float`
expectation (e.g. `binary_operation.php`'s bitwise/shift folding), that
was already hand-curated to the literal value in an earlier phase,
independent of this filter. So there was nothing to curate into the
existing fixture files.

The relaxation is still worth keeping: it stops future extractions from
silently dropping literal assertions once a literal-heavy source (5I, 5K)
is unblocked.

### 5G. Un-SKIP Psalm Assertions — DONE

The SKIP markers this phase targeted are gone and all fixture assertions
pass. Four SKIPs were reintroduced afterwards, when removing the runner's
literal-widening leniency exposed assertions PHPantom did not satisfy;
those have since been cleared as their linked items landed, so no
assertion in the ported corpus is skipped today.

---

## Phase 6: php-lsp Test Infrastructure (patterns, not test content)

php-lsp (a Rust PHP language server, https://github.com/jorgsowa/php-lsp)
has little test *content* worth porting (its type assertions are
thinner than what Phases 1-5 already cover, and it has no
framework-aware semantics to steal). What is
worth porting is two pieces of test *infrastructure* from its suite,
plus one pattern to evaluate.

### 6A. Caret-annotation DSL for diagnostics and references

**Status: Ready to port · Complexity: Medium**

php-lsp's diagnostics and references tests annotate expectations
inline in the PHP source instead of asserting positions in Rust code:

```php
$undefined->method();
// ^^^^^^^ error: Undefined variable $undefined
```

and `// ^^^ ref` / `// ^^^ def` markers for reference tests. The
harness parses the carets into expected ranges and diffs them against
server output, so a fixture is self-documenting and adding a case
never touches Rust code.

PHPantom's fixture runner (`tests/fixture_runner.rs`) already does
data-driven fixtures but position expectations live in the Rust side.
Port the caret parser into the fixture runner as an additional
assertion mode for `diagnostics_*` and reference fixtures.

**References (php-lsp's own repo layout):**
- `tests/navigation/references/basic.rs` (caret DSL usage)
- `tests/analysis/feature_diagnostics_type_errors.rs`
- `tests/common/server.rs` (the `check_*` helpers that render responses
  to stable snapshot strings)

### 6B. Wire-protocol smoke suite

**Status: Ready to port · Complexity: Medium-High**

All 9,762 PHPantom tests go through `create_test_backend()` internal
APIs, so a regression at the protocol layer (capability negotiation,
UTF-16 position encoding, response shape, `initialize` option
parsing) is invisible to the suite. php-lsp's harness spins up the
real server through `tower_lsp::LspService` over in-memory duplex
streams with real Content-Length JSON-RPC framing and drives it as an
editor would (`initialize` handshake, `didOpen`, request, assert on
the raw response).

Port the harness (~400 lines: php-lsp's `tests/common/client.rs`
`spawn_server`/`frame`/`read_msg`, and the handshake helpers in
PHPantom's `tests/common/server.rs:255-287`) and add a *small* smoke suite
(one round-trip per advertised capability, plus position-encoding
edge cases). This is not a migration target for existing tests — the
internal-API tests stay; the wire suite covers only what they cannot
see.

### 6C. Pinned-corpus location snapshots — EVALUATE

**Status: Evaluate · Complexity: Medium**

php-lsp clones real framework source (Laravel, 1,609 files; a Symfony
demo) and snapshot-tests exact `file:line:col` results for
navigation, hover, and completion against it (tests are `#[ignore]`d
and `#[serial]`, run on demand). `projects/analyze-triage.md` already
covers *diagnostics* over real codebases, but those projects are
private and the triage is manual; nothing today pins *navigation and
hover* results to exact locations on a large public corpus.

If adopted, use an already-public corpus we ship (`projects/pdepend`
or `projects/phpmd`) or a pinned Laravel framework tag, and gate the
suite behind `#[ignore]` like php-lsp does. Evaluate whether the
maintenance cost (snapshots break on corpus re-pin) is worth the
regression coverage beyond what the fixture suites already give.

### 6D. Regression probes from php-lsp's July-August 2026 bug-fix log — DONE

One probe per bug class, each added to the existing file for its feature:

| Bug class | Probes | Result |
|---|---|---|
| Semi-reserved keyword as a member name | `definition_members.rs`, `hover.rs`, `document_highlight.rs` | Already correct |
| PHPDoc-only tokens misresolving | `hover.rs`, `definition_type_hints.rs` | **Fixed:** a template used in a docblock (`@param T $x` under `@template T`) resolved to a same-named class for hover and go-to-definition |
| CRLF/Unicode range conversion | `references.rs`, `rename_member.rs` | Already correct |
| Find References from an enum case's declaration | `references.rs` | Already correct |
| `parent::CONST` owner resolution | `definition_members.rs`, `hover.rs`, `references.rs` | Already correct |

A member search counts a redeclaring child's constant as part of the same
family (as an override is for a method), so Find References on
`Base::LIMIT` also lists `self::LIMIT` in a child that redeclares it. That
is the existing design, not a `parent::` bug, and the probe asserts the
owner matching rather than exclusivity.

---

## Phase 7: zed-laravel `laravel-lsp` (Laravel behavior ports)

zed-laravel (https://github.com/mike-bronner/zed-laravel) is a
tree-sitter-based Laravel LSP for Zed (MIT, ~2,800 tests). Unlike the
type-assertion suites of Phases 1-5, its tests are *behavior specs*:
magic-member classification, string-key resolution targets, navigation
locations. They are written as inline-PHP + tempdir PSR-4 fixtures
(model source in a string, `composer.json` with a PSR-4 map, assert on
the resolved result), which maps almost 1:1 onto PHPantom's
`create_psr4_workspace()` pattern — the *content* ports cleanly even
though the harness differs.

**Trust level:** their engine is heuristic (tree-sitter pattern
matching, no type resolution), so expectations are not gospel. Verify
surprising cases against the Laravel runtime
(`examples/laravel/assertions.php`) before porting. Where their
expected behavior conflicts with type-engine-correct behavior, ours
wins — file the divergence upstream instead of porting the test.

**Porting is gap-driven, not wholesale.** `completion_laravel.rs`
alone has 253 tests with heavy overlap. For each area below, diff
their cases against our existing coverage and port only the gaps
(principle 3: skip or replace, don't duplicate).

### 7A. Direct overlap — DONE (265 tests ported, 41 ignored as known gaps)

Every module in the direct-overlap list was gap-diffed against our
coverage and only the missing cases were ported, into one file per
concern:

| File | Tests | Their module(s) |
|---|---:|---|
| `laravel_eloquent_magic.rs` | 23 | `member_resolver`, `generic_type_parsing`, `query_chain_completion_handler` |
| `laravel_route_names.rs` | 39 | `route_discovery`, `route_name_locator`, `route_outline`, `route_hover` |
| `laravel_view_names.rs` | 19 | `view_var_index`, `view_declaration_locator` |
| `laravel_config_values.rs` | 18 | `config`, `config_lookup`, `config_key_locator` |
| `laravel_translation_keys.rs` | 38 | `translation_lookup`, `vendor_translations`, `translation_key_locator`, translation namespace tests |
| `laravel_env_keys.rs` | 16 | `env_key_locator` |
| `laravel_facade_resolution.rs` | 14 | `facade_resolver`, `naming` (component tags) |
| `composer_autoload_edge_cases.rs` | 9 | `composer_autoload`, FQCN/autoload containment tests |
| `laravel_migration_columns.rs` | 9 | `migration_index` (static parsing only) |
| `blade_preprocessing.rs` | 51 | `blade_props`, `blade_embedded_php`, `blade_directive_tokens`, `blade_loops`, `blade_php_block`, loop/slot/directive context tests |
| `position_encoding_robustness.rs` | 29 | `byte_offset_panic_hardening` (from 7B), `class_locator_and_properties` |

`tests/route_binding_resolution` turned out to be Livewire-only and was
skipped. Where their heuristic expectation disagreed with the framework
(e.g. `orWhereFoo` as a dynamic finder, `$loop` inside `@while`, a
first-wins duplicate `.env` key, an unregistered `ns::` translation
namespace), the Laravel runtime's behaviour was ported instead.

The port fixed the gaps that were small enough to fix in place (route
group `as()` and replace semantics, a name ahead of the registration verb,
migration index calls read as columns, `lang_path()` namespaces, the last
duplicate `.env` value, Blade's `\B@` directive boundary, empty config
strings). The rest are filed in [bugs.md](bugs.md) (B335-B356) and L24,
and their tests are `#[ignore = "known gap: …"]`d so the suite stays
green while the gap stays visible: un-ignore a test when its entry lands.

### 7B. Partial overlap — DONE (64 tests ported, 10 ignored as known gaps)

All ten modules were gap-diffed. Most of their ~280 cases test their own
markdown rendering, helpers, or lens anchoring, or are already covered;
`vendor_member_prover` backs a dead-code diagnostic PHPantom does not have
and ported nothing. The gaps went into the existing file per concern:

| File | Tests | Their module(s) |
|---|---:|---|
| `rename_variable.rs` | 18 | `php_variable_rename` |
| `rename_symbols.rs` | 5 | `php_variable_rename`, `class_rename` (new-name validation, vendor refusal) |
| `rename_class.rs` | 2 | `class_rename` |
| `laravel_eloquent_magic.rs` | 15 | `hover`, `method_name_completion`, `completion_format` |
| `definition_laravel.rs` | 5 | `code_lens`, `references` |
| `hover.rs`, `laravel_translation_keys.rs`, `docblock_types.rs` | 3 | `hover`, `completion_format` |
| `laravel_view_names.rs`, `laravel_route_names.rs` | 5 | `references` |
| `implementation.rs`, `type_hierarchy.rs`, `src/diagnostics/cross_file.rs` | 7 | `class_hierarchy_index` |
| `document_symbols_blade.rs` | 4 | `document_symbols` |

The port fixed the gaps small enough to fix in place: variable rename leaking
into a closure without `use` and from a top-level script into functions,
`global $x` rename, invalid new names, a bare `: HasMany` relationship
losing its related model, scope/accessor/relationship docblocks on hover,
`whereId` from the implicit primary key, a `scopeWhereX` losing
go-to-definition to the `x` column, translation hover ignoring
`app.locale`, go-to-implementation from a trait's abstract method, saves
not requeueing grandchildren, and the legacy `<x-slot name="…">` outline
label. The rest are filed as B360-B366 with `#[ignore = "known gap: …"]`
tests.

Left for a maintainer decision rather than filed: code lenses for route,
config, translation, env, and view declarations; route declarations in the
`routes/*.php` outline; `@include`/`@props`/`@extends` in the Blade outline;
find-references on component tags; an "overrides" lens on a property that
redeclares a parent's; and what renaming from a `use … as Alias` alias
should rename (today it renames the class, keeping the alias).

### 7C. Skip — features PHPantom doesn't have

Not test ports; if any become sprint items, revisit their suites as
the spec: Livewire (~80 across modules), `alpine` (45),
`folio_discovery` (42), Flux/Inertia/slot navigation, `blade_var_rename`
(54), `column_rename` (18), `middleware_binding_locator` (19),
`validation_rules` (3), artisan `command_*` (~30), live-DB `database`
(94).

**Skip — their infrastructure:** `salsa_impl` (138), disk caches,
`file_watcher`, `pattern_indexer`, `reindex` — Salsa-specific, nothing
transfers.

### Phase 7: done (7C revisits per feature)

---

## Phase 8: Bladestan compiled corpora — READY (as a gap report)

**Complexity: Low for the script**

Bladestan is the PHPStan extension for Blade template analysis, and
PHPantom implements the same contract model by design. Its test suite is
ported in full.

What remains here is a different kind of material: the compiled output
Bladestan left behind in three of the sample projects, which is a gap
report rather than a test port. (If you do revisit the suite itself, work
from the full actively maintained checkout, not `projects/bladestan` —
that one is a stale, minimal copy kept only for `analyze` diagnostic
triage, per `CLAUDE.local.md`.)

It writes that output to `.bladestan/__templates__/` inside the project
it analysed, one PHP file per view, which makes the tree a ready-made
answer key for "what does this template have in scope":

| Project | Compiled views | Blade source roots |
|---|---:|---|
| `projects/luxplus-backoffice` | 559 | `resources/backoffice/views`, `resources/views` |
| `projects/luxplus-website` | 547 | `resources/webshop/views`, `resources/kiosk/views`, `resources/views` |
| `projects/luxplus-site-manager` | 5 | `resources/views` |

(These are private codebases. Keep their names in this file and out of
anything committed, and anonymize any snippet that becomes a test
fixture.)

**Pairing a compiled file with its source.**
`.bladestan/bladestan-manifest.json` is the index: `templates` maps a
view name to `{source, sourceHash, output}`, where `output` is relative
to `.bladestan/` and `source` is *absolute*. Both `source` and the
top-level `projectRoot` are paths on the machine Bladestan ran on, not
paths under `projects/`, so strip the `projectRoot` prefix off `source`
and rejoin it onto the local project directory. Every entry in all three
projects rebases cleanly that way, so a mismatch means a stale manifest
rather than a path edge case. The compiled file's own
`// @bladestan-source:` header carries the same absolute path and needs
the same treatment.

**The answer key** is the run of `/** @var T $x */` lines between that
header and the first statement, in Bladestan's own priority order:
signature, then component-body scope, then view-composer data, then
shared variables. `$errors` and `$__env` correspond to what our prologue
declares; `$app` appears in all 1,111 compiled files and is Bladestan
reading the view factory's shared variables.

**What to expect from a run.** The last comparison predates
backing-class member injection, shared/composer variables, and `$this` in
a Livewire view, all of which supplied the bulk of what it flagged, so
the run is worth repeating from scratch rather than reading against those
numbers. The known gap it will *not* catch is
[BL22](blade.md#bl22-a-template-never-learns-anything-from-the-templates-that-render-it):
a partial rendered only from other templates still gets nothing from
their `@include`/`@each` call sites.

**Caveats when reading a difference as a defect.** Bladestan strips the
signature docblock and re-emits it, so its `@var` block is the *whole*
scope; ours is split between the prologue and the docblock left in the
template, and the comparison has to account for both. `@extends` does
not contribute to the body in either tool (it is a call-site concern),
so a parent layout's variables are legitimately absent from a child's
block. And the trees are only as fresh as the last Bladestan run: check
`sourceHash` against the local file before trusting an entry.

If the comparison becomes a checked-in script under `scripts/`, it has to
take the project directory as an argument. Hardcoding any path above
would put a private project name in the public repo.

---

## Phase 9: PHPStan `nsrt/` sweep — DONE (344 files, 2,496 running assertions)

Every file in PHPStan's `tests/PHPStan/Analyser/nsrt/` (1,425, including the
`foreach/`, `json-decode/`, `throw-points/`, and `traits/` subdirectories)
was run through the runner, and every file that passed at least 60% of at
least four assertions was ported into `tests/phpstan_nsrt/`, which went from
32 to 376 files.

- **Ported verbatim (223 files):** every assertion passes. Files whose only
  assertions are `mixed`/`*ERROR*` were left out, since those pass even when
  hover returns nothing.
- **Curated (121 files):** each failing assertion was triaged. Out-of-scope
  ones under the type policy above were deleted (285), spellings PHPantom
  prints differently but equivalently, or answers where it is the more
  precise of the two, were rewritten with a comment (15), and real gaps are
  `// SKIP` (154) and filed as B368-B411 in [bugs.md](bugs.md).

Porting fixed the gaps small enough to fix in place: a `@return` member the
native return type rules out (`static|false` on `: DateTimeImmutable`, a
`null` on a non-nullable return, the array half of
`IteratorAggregate::getIterator()`), a `static`-typed property or `__get()`
read through a variable, `func_get_args()`, a `(bool)` cast on `preg_match()`,
a `self` hint shown instead of its class, a by-reference inference pass that
cost statements × locals, and the stub version filter rescanning each stub
file per symbol (the rest of that is P66).

**Left for follow-up:**

- `if.php` is curated but not ported: it takes 53 s in a debug build until
  [P65](performance.md#p65-every-call-site-repeats-the-full-function-lookup-hit-or-miss)
  lands. The curated copy is easy to regenerate from the upstream file.
- `properties.php` needs its companion `Analyser/data/properties-defined.php`,
  which declares the class it reads, and the two use different imports under
  the same unbraced namespace.
- The ~1,000 files below the 60% line, most of them blocked on Phase 5
  features or dominated by out-of-scope types.
- **`tests/PHPStan/Analyser/data/`** is a second assertType corpus
  (`NodeScopeResolverTest` yields it alongside `nsrt/`), not yet surveyed.

---

## Existing Test Evaluation

PHPantom has ~14,460 tests (`#[test]` + `#[tokio::test]` functions,
counted 2026-09-23 — up from the 9,762 this section previously recorded;
re-count before trusting these numbers again, since they drift fast).
When porting tests from reference projects, compare against existing
tests in these high-overlap areas:

| Area | Existing tests | Expected overlap with ports |
|------|---------------|---------------------------|
| `completion_variables.rs` | 291 | High overlap with Mago narrowing tests |
| `completion_guard_clauses.rs` | 29 | Moderate overlap with Mago narrowing |
| `completion_generics.rs` | 147 | High overlap with Mago + PHPStan generics |
| `completion_laravel.rs` | 302 | High overlap with Larastan tests |
| `completion_mixins.rs` | 34 | Moderate overlap with Mago magic members |
| `completion_array_shapes.rs` | 118 | Moderate overlap with Mago + PHPStan arrays |
| `completion_ternary.rs` | 24 | Some overlap with Mago ternary tests |
| `completion_match_expression.rs` | 12 | Some overlap with Mago match tests |
| `completion_enums.rs` | 40 | Some overlap with PHPStan enum tests |
| `hover.rs` | 430 | High overlap with PHPStan assertType tests |
| `diagnostics_type_errors.rs` | 474 | Moderate overlap with Mago diagnostics |
| `diagnostics_unknown_members.rs` | 394 | Some overlap with Mago null-access tests |

---

## Summary

| Phase | Source | Status |
|-------|--------|--------|
| 1 | Mago (easy) | **Done** |
| 2 | Larastan (easy) | **Done** |
| 3 | PHPStan (actionable) | **Done** |
| 3.5 | Psalm (relevant + partial) | **Done** |
| 4A | Mago issue regressions (22 patterns) | **Done** (14/22 ported, 8 blocked on non-test-porting work — see T42/T43/C13 and Phase 5) |
| 4B | Trait resolution (Mago) | **Done** |
| 4C | Psalm non-assertion patterns (8 files) | **Done** (7/8 ported, SKIPs pending B343-B347/T26) |
| 5G | Un-SKIP Psalm assertions | **Done** |
| 5A-5F, 5H-5K | Requires new features (~10 groups) | Blocked (see sections above) |
| 5L | Extract Psalm's literal-typed assertions | **Done** |
| 6A | php-lsp caret-annotation DSL (infrastructure) | Ready |
| 6B | php-lsp wire-protocol smoke suite (infrastructure) | Ready |
| 6C | Pinned-corpus location snapshots | Evaluate |
| 6D | Regression probes from php-lsp's bug-fix log (5 bug classes) | **Done** (1 bug fixed, 4 already correct) |
| 7A | zed-laravel direct-overlap behavior ports | **Done** (265 tests, 41 ignored pending B335-B356/L24) |
| 7B | zed-laravel partial-overlap areas | **Done** (64 tests, 10 ignored pending B360-B366) |
| 7C | zed-laravel feature-gap suites | Skip (revisit per feature) |
| 8 | Bladestan compiled corpora as an answer key | Ready to re-run (watch for BL22 leftovers); the test suite itself is fully ported |
| 9 | PHPStan `nsrt/` sweep with full stubs | **Done** (344 files, 154 SKIPs pending B368-B411; `if.php` waits on P65) |
| **Remaining** | | 6A/6B infrastructure, 6C evaluation, 8 re-run, PHPStan `Analyser/data/` survey |

The session estimates assume one session = one focused work block where
the agent ports tests, runs CI, and resolves failures. Actual time will
vary based on how many existing tests overlap (reducing work) and how
many type engine bugs the ported tests uncover (increasing work, but
that is the whole point).