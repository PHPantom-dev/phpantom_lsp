# PHPantom — Diagnostics

Items are ordered by **impact** (descending), then **effort** (ascending)
within the same impact tier.

| Label      | Scale                                                                                                                  |
| ---------- | ---------------------------------------------------------------------------------------------------------------------- |
| **Impact** | **Critical**, **High**, **Medium-High**, **Medium**, **Low-Medium**, **Low**                                           |
| **Effort** | **Low** (≤ 1 day), **Medium** (2-5 days), **Medium-High** (1-2 weeks), **High** (2-4 weeks), **Very High** (> 1 month) |

---

## Severity philosophy

PHPantom assigns diagnostic severity based on runtime consequences:

| Severity        | Criteria                                                                                                                                                                                                                                                                                                                                                                                     | Examples                                                                                                                                                                                                                                                                      |
| --------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Error**       | Would crash at runtime. The code is definitively wrong.                                                                                                                                                                                                                                                                                                                                      | Member access on a scalar type (`$int->foo()`). Calling a function that doesn't exist (`doesntExist()`).                                                                                                                                                                      |
| **Warning**     | Likely wrong but could work for reasons we can't verify statically. The types are poor but the code might be correct at runtime.                                                                                                                                                                                                                                                             | Accessing a member that doesn't exist on a non-final class (`$user->grantAccess()` where `User` has no such method but a subclass might). Unknown class in a type position (`Class 'Foo' not found`). Subject type resolved to an unknown class so members can't be verified. |
| **Hint**        | The codebase lacks type information. Off by default or very subtle. Poorly typed PHP is so common that showing these by default would be noise for most users. Anyone who does care about type safety is likely running PHPStan already. Unless our engine becomes very strong, these diagnostics either expose our own inference gaps or bother users who never opted into static analysis. | `mixed` subject member access (opt-in via `unresolved-member-access`). Deprecated symbol usage (rendered as strikethrough).                                                                                                                                                   |
| **Information** | Advisory. Something the developer might want to know.                                                                                                                                                                                                                                                                                                                                        | Unused `use` import (rendered as dimmed). Unresolved type in a PHPDoc tag.                                                                                                                                                                                                    |

---

## D3. Deprecated rendering — chain subject resolution

**Impact: Low-Medium · Effort: Medium**

Chain subjects like `getHelper()->deprecatedMethod()` do not produce
a deprecated diagnostic because `resolve_subject_to_class_name` in
`diagnostics/deprecated.rs` returns `None` for non-variable,
non-keyword subjects (the `_ => None` arm). The function call return
type is never resolved, so the member deprecation check is skipped.

**Fix:** Route chain subjects through the completion/type-inference
pipeline to resolve the return type of the call before checking the
member for deprecation. The variable-resolution path already works
for `$var->deprecatedMethod()` via `resolve_variable_subject`; the
gap is function-call and method-call return types in subject position.

The following have been verified and are covered by tests:

- Deprecated class references in `new`, type hints, `extends`, and
  `implements` positions all render with strikethrough.
- Deprecated method calls, property accesses, and constants render
  with strikethrough (via both `$var->` and `ClassName::` subjects).
- Offset-based class resolution for `$this`/`self`/`static` resolves
  to the correct class in files with multiple class declarations.

---

## D5. External tool diagnostic suppression actions

**Impact: Low · Effort: Low (per tool, after proxy exists)**

PHPantom's own inline suppression (`// @phpantom-ignore code`) has
shipped. PHPStan suppression is also implemented ("Ignore PHPStan
error" / "Remove unnecessary @phpstan-ignore"). What remains is
wiring up suppression actions for additional external tool proxies:

- PHPCS: `// phpcs:ignore [Sniff.Name]` or `// phpcs:disable` /
  `// phpcs:enable` blocks.
- PHPMD (3.0): `#[SuppressWarnings(RuleName::class)]` as a PHP attribute.

Each tool needs its diagnostic proxy before its suppression action
can be wired up (D10 for PHPMD; PHPCS proxy is not yet filed).

---

## D6. Unreachable code diagnostic

**Impact: Low-Medium · Effort: Low**

Dim code that appears after unconditional control flow exits:
`return`, `throw`, `exit`, `die`, `continue`, `break`. This is a
Phase 1 (fast) diagnostic since it requires only AST structure, not
type resolution.

### Behaviour

| Scenario                                           | Rendering                           |
| -------------------------------------------------- | ----------------------------------- |
| Code after `return $x;` in same block              | Dimmed (DiagnosticTag::UNNECESSARY) |
| Code after `throw new \Exception()`                | Dimmed                              |
| Code after `exit(1)` or `die()`                    | Dimmed                              |
| Code after `continue` or `break` in a loop         | Dimmed                              |
| Code after `if (...) { return; } else { return; }` | Dimmed (both branches exit)         |

Severity: **Hint** with `DiagnosticTag::UNNECESSARY` so editors dim
the text rather than underlining it. This matches how unused imports
are rendered.

### Implementation

Walk the AST statement list. After encountering a statement that
unconditionally exits the current scope (return, throw, expression
statement containing `exit`/`die`), mark all subsequent statements in
the same block as unreachable. The span covers from the start of the
first unreachable statement to the end of the last statement in the
block.

Phase 1 only handles the simple single-block case. Whole-branch
analysis (both if/else branches exit) is a future refinement.

### Debugging value

When our type engine silently resolves a method to a `never` return
type (e.g. an incorrectly resolved overload), unreachable code after
the call becomes visible, signalling the bug.

---

## D10. PHPMD diagnostic proxy

**Impact: Low · Effort: Medium**

Proxy PHPMD (PHP Mess Detector) diagnostics into the editor, following
the same pattern as the existing PHPStan proxy. PHPMD 3.0 (once
released) is the target version. It will get a `[phpmd]` TOML section
with `command`, `timeout`, and tool-specific options mirroring the
`[phpstan]` schema.

### Prerequisites

- PHPMD 3.0 must be released. Current 2.x output formats and rule
  naming may change.
- The diagnostic suppression code action (D5) can add PHPMD's
  `@SuppressWarnings(PHPMD.[RuleName])` syntax once the proxy exists.

### Implementation

1. Add a `[phpmd]` section to the config schema in `src/config.rs`
   with `command` (default `"vendor/bin/phpmd"`), `timeout`, and
   an `enabled` flag.
2. Run PHPMD with XML or JSON output on the current file (or changed
   files) and parse the results into LSP diagnostics.
3. Map PHPMD rule names to diagnostic codes so that suppression
   actions (D5) can insert the correct `@SuppressWarnings` annotation.
4. Respect the same debounce and queueing logic used by the PHPStan
   proxy to avoid overwhelming the tool on rapid edits.

---

## D13. Unify diagnostic subject resolution with completion/hover

`unknown_members.rs` has two secondary resolvers that run their own
independent type resolution when `resolve_target_classes_expr` returns
empty:

- `resolve_scalar_subject_type` (~130 lines) re-resolves variables,
  property chains, and call expressions to detect scalar types.
- `resolve_unresolvable_class_subject` (~80 lines) re-resolves
  variables and call expressions to detect class names that can't be
  loaded.

Both duplicate logic from `resolver.rs` and
`variable/resolution.rs` but can diverge, producing diagnostics for
types that completion and hover cannot see (or vice versa).

### Goal

The diagnostic path should use the same resolution result that
completion and hover use. All three consumers should see identical
outcomes for the same subject text at the same cursor position.

### Approach

Extend the shared resolver's return type (or add a secondary result)
to carry scalar type information and unresolvable class names
alongside the resolved `ClassInfo` list. The diagnostic collector
would then inspect this enriched result instead of running its own
resolution. This eliminates the secondary resolvers entirely.

### Files

- `src/diagnostics/unknown_members.rs` — remove
  `resolve_scalar_subject_type` and `resolve_unresolvable_class_subject`
- `src/completion/resolver.rs` — enrich the resolution result

---

## D14. Tighten argument type mismatch diagnostic (Phase 2)

**Impact: High · Effort: Medium**

`is_type_compatible` in `src/diagnostics/type_errors.rs` silences
several cases that are genuine bugs at runtime. Phase 1 was
intentionally permissive to avoid false positives while the engine
matured; Phase 2 tightens the remaining gaps. PHPStan and Psalm
already flag most of these.

### 1. Nullable arg → non-nullable param (lines 264–271)

Currently silenced with a MAYBE comment ("developer may have guarded
against null"). This is the #1 source of runtime `TypeError` in
PHP 8+. Both PHPStan and Psalm flag it. Should be reported at least
as **Warning** severity, since the null path may be unguarded.

### 2. `void` as argument (lines 94–96)

Currently silenced conservatively. Passing the return value of a
`void` function is always a bug — PHP 8 returns `null` but the call
site clearly misunderstands the API. Should be **Error** severity.

### 3. Union any-member-compatible threshold (lines 189–213)

Currently: if ANY single member of an arg union is compatible with
the param, the entire union passes. Combined with the other
permissive rules above, this creates cascading permissiveness (e.g.
`null|BadType` passes a `string` param because `null` is not
checked, then `BadType` is the "any" member that gets skipped).
Consider requiring all non-null members to be compatible, or at
least flagging when a majority of members are incompatible.

### 5. Reverse hierarchy acceptance (Direction 2)

Currently: when the arg type is a *supertype* of the param type
(e.g. `CarbonInterface` passed to `Carbon`), the diagnostic is
silenced for all non-final classes because "the value *might* be
the narrower type at runtime." This means the diagnostic can only
catch type errors between completely unrelated classes, which
severely limits its value. Passing `Animal` where `Dog` is expected
is silently accepted.

This is the single largest gap in the diagnostic. Tightening it
requires control-flow analysis (instanceof guards, assert calls) to
know whether the broader type was actually narrowed before the call
site. Without CFA, the false positive rate would be high. Consider
reporting at **Warning** severity with a message like "argument type
`Animal` is broader than expected `Dog`; verify the value was
narrowed before this call."

---

## D15. Unused parameter diagnostic

**Impact: Low · Effort: Low**

Flag function and method parameters that are never read inside the
body. This was intentionally excluded from D4 (unused variable
diagnostic) because false positives are common for callbacks, interface
implementations, and framework conventions (e.g. Laravel event
listeners) that require specific parameter signatures even when not
all parameters are used. Users can now silence false positives with
`// @phpantom-ignore unused_parameter`.

### Scope

1. Function and method parameters (including closures and arrow
   functions) that are never read inside their body.
2. Constructor parameters that are not promoted and never read.

### Exclusions

- Parameters named `$_` or starting with `$_` (intentional discard).
- Promoted constructor parameters (they are property assignments).
- Parameters in abstract methods and interface method signatures
  (no body to check).


