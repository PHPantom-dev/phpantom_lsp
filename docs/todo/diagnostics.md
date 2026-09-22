# PHPantom — Diagnostics

Items are ordered by **impact** (descending), then **complexity** (ascending)
within the same impact tier.

| Label      | Scale                                                                                                                  |
| ---------- | ---------------------------------------------------------------------------------------------------------------------- |
| **Impact** | **Critical**, **High**, **Medium-High**, **Medium**, **Low-Medium**, **Low**                                           |
| **Complexity** | **Low** (mechanical/boilerplate, no design decisions), **Medium** (self-contained, follows an existing pattern), **Medium-High** (spans modules, some new design), **High** (shared/core subsystem, correctness or performance tradeoffs), **Very High** (cross-cutting architecture, wide blast radius) |

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

## D5. External tool diagnostic suppression actions

**Impact: Low · Complexity: Low (per tool, after proxy exists)**

PHPantom's own inline suppression (`// @phpantom-ignore code`) has
shipped. PHPStan suppression is also implemented ("Ignore PHPStan
error" / "Remove unnecessary @phpstan-ignore"). The PHPCS proxy itself
has also shipped (`src/diagnostics/external/phpcs.rs`, `[phpcs]` config
section), but nothing wires up a suppression action for it yet. What
remains is wiring up suppression actions for additional external tool
proxies:

- PHPCS: `// phpcs:ignore [Sniff.Name]` or `// phpcs:disable` /
  `// phpcs:enable` blocks. The proxy exists; only the suppression
  action is missing.
- PHPMD (3.0): `#[SuppressWarnings(RuleName::class)]` as a PHP
  attribute. Blocked on the proxy itself (D10).

---

## D6. Unreachable code diagnostic

**Impact: Low-Medium · Complexity: Medium**

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

**Impact: Low · Complexity: Medium**

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

## D15. Unused parameter diagnostic

**Impact: Low · Complexity: Medium**

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

---

## D16. `unreachable_match_arm` ignores literal subject types

**Impact: Low-Medium · Complexity: Medium**

`scalar_type_label` in `src/diagnostics/match_type_errors.rs` answers
`None` for a literal type (`'exception'`, `42`), so a subject the
resolver typed as one exact value never reaches the arm check and no
arm is ever reported unreachable. The comment there explains why: a
literal was as often what survived after resolution lost an
alternative it could not type as it was a genuine one-value subject,
and taking the claim without that evidence produced false positives.

The resolver no longer loses those alternatives. An unresolvable
branch now widens the union it belongs to instead of dropping out of
it, so a literal that reaches this diagnostic is a claim the resolver
stands behind.

**Fix:** Give `TypeKind::Literal` its scalar kind in
`scalar_type_label` and check the resulting arms. Cover the case the
old comment was guarding against with a test: a subject whose other
branch cannot be typed must still produce no diagnostic, because it
now resolves to `mixed` rather than to the surviving literal.



---

## D17. `docblock_native_mismatch` only judges nullability

**Impact: Low · Complexity: Medium-High**

```php
/** @param int $name */
function greet(string $name): void {}    // not flagged: int is not string at all

/** @param Foo $value */
function take(Bar $value): void {}       // not flagged: `Foo` may alias `Bar`
```

`src/diagnostics/docblock_native_mismatch.rs` compares a documented type
against its native hint on one axis only: whether the annotation admits a
`null` the signature rules out. A documented type that is not a subtype of
the native hint on any *other* axis (`@param int` on a `string`, `@return
array` on a `: string`) stays silent. That is the check PHPStan's
`IncompatiblePhpDocTypeRule` performs, and the one the existing
`is_type_compatible` in `src/diagnostics/type_errors/compatibility.rs`
already has the machinery for.

Covering it means resolving the documented type's bare names first, since a
`Foo` that a `@template` list or an imported `@psalm-type` alias stands
behind may well be the native `Bar` after resolution. The nullability axis
sidesteps that today: a name that resolves to something nullable never
spells `null` itself, so the check simply does not fire on it.

**Fix:** Resolve the documented type's names against the declaration's own
`@template` list, the enclosing class's, and the file's
`@psalm-type`/`@psalm-import-type` tags, then run the comparison through
`is_type_compatible` rather than the nullability test alone.

---

## D18. `array<int, T>` is accepted wherever a `list<T>` is declared

**Impact: Low · Complexity: Medium-High**

```php
/**
 * @param list<int> $values
 * @return list<int>
 */
function keep(array $values): array {
    return array_filter($values, fn ($v) => $v > 3);  // array<int, int>, not flagged
}
```

`is_type_compatible` in `src/diagnostics/type_errors/compatibility.rs`
carries an explicit MAYBE hatch for `array<int, X>` reaching a `list<X>`
parameter or return type, on the grounds that PHP codebases spell the two
interchangeably. The core `is_subtype_of` already rejects the direction
(only `list<X>` satisfies `array<int, X>`, not the reverse), so the hatch
is the only thing standing between us and PHPStan's report here.

Now that `array_filter()` reports the `array<int, T>` it actually
produces, the hatch is what keeps the second half of the over-claim
alive: a function that hands back an unwrapped filter result still
passes a declared `list<T>`.

**Fix:** Drop the `array<int, X> → list<X>` arm and audit the corpus
under `projects/` for what it starts reporting. The arm exists because
plain `array<int, X>` is what an unannotated array resolves to in many
places, so retiring it wants the resolver to answer `list<X>` for the
shapes that genuinely are lists (literal arrays, `array_values()`,
appended-to locals) first. Pay for it with resolver precision, the same
way the supertype-where-subtype hatch was retired.

---

## D19. `invalid_member_access` cannot tell a property read from a write

**Impact: Low-Medium · Complexity: Medium**

PHP dispatches an unreachable property to a different magic method
depending on what is being done to it: `__get` for a read, `__set` for a
write, `__isset` for `isset()`, and `__unset` for `unset()`. The span the
visibility check runs on records only that a property was accessed, so
the check cannot pick the handler that actually applies and treats the
presence of any of the four as reason enough to stand down.

The result is a missed report rather than a wrong one. A class that
declares `__set` but no `__get` silences a read it would in fact fatal
on, and the same holds for every other mismatched pairing.

**Fix:** Carry the operation on the `MemberAccess` span — read, write,
`isset`, `unset` — the way `readonly_writes.rs` recovers write targets
from the AST, and require the handler that matches it. The same
information would let the readonly check drop its own separate walk.

---

## D20. `Foo::$bar` and `Foo::bar` are the same span

**Impact: Low · Complexity: Medium**

Symbol extraction strips the `$` from a static property access
(`member_access.rs`) and records only a name plus `is_static`, so a
static property and a class constant of the same name are
indistinguishable downstream. Every consumer has to guess a precedence;
the visibility check tries the constant first.

Two classes of mistake follow, both needing a class that declares a
constant and a static property under one name:

```php
class Collision {
    private const token = 'constant';
    public static string $token = 'property';
}

echo Collision::$token;   // reported as a private constant
```

and the reverse pairing, where a genuinely unreachable static property
is passed off as a public constant and nothing is reported.

**Fix:** Record the member kind the syntax actually names — instance
property, static property, constant, or method — rather than a name and
a static flag.

---

## D21. A union of an unreachable and a missing member is reported by neither check

**Impact: Low · Complexity: Medium-High**

`Known|Other` where `Known::$x` is private and `Other` has no `$x` at all
fails on every runtime branch, and nothing reports it. Both checks are
conservative in the same direction and each assumes the other covers what
it declines to judge: the unknown-member check stays silent because the
member exists on one branch, and the visibility check stays silent
because it does not exist on the other.

A related gap sits one layer earlier: the resolver hands the diagnostics
only the branches it could load and drops the rest, so neither check can
see that a union had a branch it failed to resolve. Any verdict that
wants to distinguish "no branch permits this" from "we could not read
one of the branches" needs that information preserved.

**Fix:** Give the branches of a resolved union a verdict of their own —
accessible, inaccessible, missing, unresolved — instead of collapsing
them to a list of loaded classes, and decide the union once from those.

---

## D22. Member provenance is recomputed instead of recorded

**Impact: Medium · Complexity: Medium-High**

The inheritance merge knows which class each member it folds in came
from — `merge_traits_into()` is handed the host FQN — and drops that on
the floor. `MethodInfo`, `PropertyInfo`, and `ConstantInfo` have no field
for it, so the assembled class says what a class *has* and never who
declared it.

Every check that needs the declaring class therefore recomputes it by
walking the raw hierarchy, and every such walk is a partial
reimplementation of the merge's own rules. `invalid_member_access` has
one, and it is already known to be incomplete: it does not apply
`trait_aliases`, so a member a `use` clause renamed is untraceable and
the access goes unreported.

```php
trait Opens { public function open(): void {} }

class Vault { use Opens { open as private hidden; } }

class Intruder {
    public function probe(Vault $vault): void
    {
        $vault->hidden();   // fatal at runtime, reported by nothing
    }
}
```

The access has to be written inside a class to show the gap. From
outside every class the check reports anyway, because no scope can reach
a non-public member whoever declared it, and it falls back to naming the
receiver when the declaration cannot be traced.

**Fix:** Record the declaring class during the merge and let the checks
read it, rather than each one re-deriving the merge's rules. The cost is
a field on every member across the whole index, so it wants measuring
against the memory the index already uses before it is committed to.

---

## D23. A rebound closure's scope is added to the lexical one rather than replacing it

**Impact: Low-Medium · Complexity: Medium**

A closure can run with a different class as its scope than the one it is
written in — Laravel's `Macroable`, anything through `Closure::bind`, and
anything a `@param-closure-this` tag describes. The resolver already
works this out. The visibility check does not read it; it infers from the
subject text that `$this`, `self`, or `static` names a binding and adds
the receiver as an *additional* scope, keeping the enclosing class as
well.

Adding rather than replacing is safe in the direction that matters — it
cannot invent a report — but it hides one:

```php
class Owner {
    private static function hidden(): void {}

    public function boot(): void
    {
        Target::macro('x', function (): void {
            self::hidden();      // fatal: the closure's scope is Target
        });
    }
}

class Target extends Owner {
    /** @param-closure-this static $macro */
    public static function macro(string $name, callable $macro): void {}
}
```

The closure is written inside `Owner`, so the enclosing class permits the
call, while at runtime the scope is `Target` and a parent's private
member is out of reach.

**Fix:** Take the bound class from the resolver, which already computes
it, and let it replace the enclosing class rather than joining it.
Inferring a binding from the spelling of the subject is guessing at
something the type engine has already decided.
