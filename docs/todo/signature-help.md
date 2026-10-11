# PHPantom — Signature Help

Signature help is architecturally solid. Dual-path detection (AST-based
`CallSite` lookup + text-based fallback), precomputed comma offsets for
active parameter tracking, content patching for unclosed parens, and
chain/constructor/first-class-callable resolution all work well. The
popup shows a compact parameter list with native PHP types, a shortened
return type, per-parameter `@param` descriptions, and default values in
parameter labels.

The remaining work requires new extraction or deeper protocol support.

Items are ordered by **impact** (descending), then **complexity** (ascending)
within the same impact tier.

| Label      | Scale                                                                                                                  |
| ---------- | ---------------------------------------------------------------------------------------------------------------------- |
| **Impact** | **Critical**, **High**, **Medium-High**, **Medium**, **Low-Medium**, **Low**                                           |
| **Complexity** | **Low** (mechanical/boilerplate, no design decisions), **Medium** (self-contained, follows an existing pattern), **Medium-High** (spans modules, some new design), **High** (shared/core subsystem, correctness or performance tradeoffs), **Very High** (cross-cutting architecture, wide blast radius) |

---

## S2. Closure / arrow function parameter signature help
**Impact: Medium · Complexity: Medium**

Signature help should work when invoking a variable that holds a closure
or arrow function:

```php
$format = fn(string $name, int $age): string => "$name ($age)";
$format('Alice', 30);  // ← signature help here
```

#### Current state

`extract_callable_target_from_variable` (`signature_help.rs`) handles
first-class callables (`$fn = makePen(...)`) by looking the variable up
in the first-class-callable assignments collected from the AST
(`parser::with_fcc_assignments`). Closures and arrow functions assigned
to variables are not collected there, so they get no signature help.

#### Implementation

1. **Resolve the variable's type** — ask the shared resolver for the
   variable's type at the call site. A closure or arrow function
   assignment resolves to a `Closure(...)` type carrying the parameter
   and return types, so no separate parse of the closure is needed and
   any closure the type engine understands (including ones passed
   through other variables) gets signature help.

2. **Build the signature** — turn the callable type into a
   `ResolvedCallableTarget` inline, without going through class
   resolution (closures don't have classes). Parameter names come from
   the callable type when it carries them, otherwise from the closure's
   AST.

3. **Label prefix** — use `$format` (the variable name) or the closure's
   inferred signature as the label prefix.

#### Tests

- Integration test: `$fn = fn(string $x): int => 0; $fn(` → assert
  signature help shows `string $x` with return type `int`.
- Integration test: `$fn = function(int $a, int $b): int { ... }; $fn('x', ` →
  assert `active_parameter` is 1.
- Integration test: `$fn = $obj->method(...)` (existing first-class
  callable path) → continues to work unchanged.

---

## S3. Multiple overloaded signatures
**Impact: Low · Complexity: Medium-High**

Some PHP functions have multiple signatures depending on argument count
or types.  For example, `array_map` can be called as:

```php
array_map(callable $callback, array $array): array
array_map(null, array ...$arrays): array
```

The LSP protocol supports returning multiple `SignatureInformation`
entries with an `activeSignature` index.  Today we return a single
signature.

#### Current state

phpstorm-stubs define multiple function entries (or parameter variants
annotated with `#[PhpStormStubsElementAvailable]`) for overloaded
functions.  Our PHP-version filtering selects one variant.  We don't
model true overloads.

#### Implementation

This is a deeper change:

1. When a function has multiple stub entries (or when a class has
   multiple `__construct` signatures for different PHP versions),
   collect all applicable signatures.
2. Return them all in the `signatures` array.
3. Set `activeSignature` based on argument-count matching: pick the
   first signature whose parameter count accommodates the current
   argument count.

**Deferred** — the single-signature approach covers 99% of real usage.

---

## S4. Named argument awareness in active parameter
**Impact: Low · Complexity: Medium**

When the user types a named argument (`callback: ` in `array_map(callback: `),
the active parameter should highlight the `$callback` parameter regardless
of its positional index.

#### Current state

Active parameter is computed purely by counting commas before the cursor.
Named arguments are handled by the named-argument completion system
(`completion/named_args.rs`) but the signature help active-parameter
tracking doesn't consult argument names.

#### Implementation

1. In `detect_call_site_from_map`, after computing the comma-based
   `active` index, extract the text of the current argument segment.
2. If the segment matches `identifier:` (named argument syntax), look up
   which parameter index corresponds to that name.
3. Override `active_parameter` with the named parameter's index.

This requires access to the resolved parameters (to map name → index),
which isn't available in the detection layer.  The override could be
applied later in `resolve_signature`, after `resolve_callable` returns
the parameter list.

---

## S5. Language construct signature help and hover
**Impact: Low · Complexity: Medium**

PHP language constructs that use parentheses (`unset()`, `isset()`, `empty()`,
`eval()`, `exit()`, `die()`, `print()`, `list()`) are not function calls in the
AST. Mago parses them as dedicated statement/expression nodes (e.g.
`Statement::Unset`) with no `ArgumentList`, so no `CallSite` is emitted and
neither signature help nor hover fires inside their parentheses. The phpstorm-stubs
don't define them either since they are keywords, not functions.

Supporting them requires emitting synthetic `CallSite` entries from the
statement-level extraction in `symbol_map/extraction/statements.rs` and adding
hardcoded parameter metadata (e.g. `unset(mixed ...$vars): void`) in
`resolve_callable`. Hover would need a similar hardcoded lookup.
