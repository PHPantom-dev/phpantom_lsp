# PHPStan Code Actions

Code actions that respond to PHPStan diagnostics. Each action parses the PHPStan
error message, extracts the relevant information, and offers a quickfix that
modifies the source code to resolve the issue.


## Prerequisites — Infrastructure improvements

No outstanding items.

---

## Tier 1 — Simple message parsing

### H4. `assign.byRefForeachExpr` — Unset by-reference foreach variable

**Identifier:** `assign.byRefForeachExpr`
**Tip (in message):** `Unset it right after foreach to avoid this problem.`

The diagnostic is on a line that uses a variable that was previously bound as a
by-reference foreach variable. The fix is to insert `unset($var);` after the
foreach loop that created the binding.

**Implementation steps:**

1. The message doesn't include the variable name, so find the variable
   on the diagnostic line in the AST.
2. Walk back to the `foreach` statement that binds it by reference
   (`&$var`).
3. Insert `unset($var);` on the line after that foreach's end (its
   closing `}` or `endforeach;`), with matching indentation.

**Stale detection:** `unset($var)` appears between the foreach closing brace
and the diagnostic line.

---

## Tier 2 — Requires locating related code

### H13. `property.notFound` (same-class) — Declare missing property

**Identifier:** `property.notFound`
**Message:** `Access to an undefined property Foo::$bar.`

For the PHPStan diagnostic, parse class name and property name from the
message: `Access to an undefined property (.+)::\$(.+)\.$`

PHPantom's own unknown-member diagnostic fires on the same code, so build
this as one native "Declare property" action that attaches to both that
diagnostic and PHPStan's `property.notFound`, not a PHPStan-only one. It
is the property counterpart of
[A40](actions.md#a40-generate-method-from-call) (generate method from
call) and should share its target-class and insertion logic.

Scope to same-file only: when the diagnostic is on `$this->bar`, the fix
targets the current class. When it references a different class, skip.

Offer two quickfixes:

1. **Declare property** — insert a property declaration at the top of the
   class body, after existing property declarations. Use `private` visibility
   and `mixed` type by default. If the diagnostic is on an assignment like
   `$this->bar = expr;`, we might infer a better type later, but start with
   `mixed`.
2. **Add `@property` PHPDoc** — add `@property mixed $bar` to the class
   docblock. Better for classes that use `__get`/`__set`.

**Stale detection:** the class now declares `$bar` as a property, or the
class docblock contains `@property ... $bar`.

**Reference:** https://phpstan.org/blog/solving-phpstan-access-to-undefined-property

---

### H15. Template bound from tip — Add `@template T of X`

**Identifiers:** various (`generics.*`, `phpDoc.*` — needs investigation)
**Tip (in message):** `Write @template T of X to fix this.`

Parse the `@template` declaration from the tip using:
`Write (@template .+ of .+) to fix this\.`

Insert the `@template` tag into the class or function docblock (create one
if needed). Same docblock insertion pattern as `add_throws.rs`.

**Stale detection:** the docblock now contains the extracted `@template` tag.

---

### H16. `match.unhandled` — Add missing match arms

**Identifier:** `match.unhandled`
**Message:** `Match expression does not handle remaining value(s): {types}`

Parse the remaining value(s) from the message:
`does not handle remaining value\(s\): (.+)$`

The value list is comma-separated. Each value can be:
- An enum case: `Foo::Bar` — generate `Foo::Bar => TODO`
- A string literal: `'foo'` — generate `'foo' => TODO`
- An int literal: `42` — generate `42 => TODO`
- A type name: `int` — generate `default => TODO` (catch-all)

Find the match expression on the diagnostic line. Locate its closing `}`.
Insert new arms before the closing `}` with correct indentation.

Use `throw new \LogicException('Unexpected value')` as the arm body, or
a `TODO` comment — configurable later.

**Stale detection:** difficult without re-parsing the match. Skip for now.

---

## Tier 3 — Removals

### H19. `property.unused` / `method.unused` — Remove unused member

**Identifiers:** `property.unused`, `method.unused`, `classConstant.unused`

Remove the whole declaration, including its docblock and attributes.
Also expose it through `phpantom_lsp fix --with-phpstan` as the
`phpstan.property.unused` / `phpstan.method.unused` rules (see the
PHPStan integration notes in [fix-cli.md](fix-cli.md#phpstan-integration)).

**Stale detection:** the class no longer declares the member.

---

### H23. `instanceof.alwaysTrue` — Remove redundant `instanceof` check

**Identifier:** `instanceof.alwaysTrue`

Drop the check: replace the `instanceof` expression with `true` when
it is part of a larger condition, or unwrap the `if` and keep its body
when it is the whole condition (an `else` branch is dead and goes with
it).

**Stale detection:** the diagnostic line no longer contains `instanceof`.

---

### H24. `catch.neverThrown` — Remove unnecessary catch clause

**Identifier:** `catch.neverThrown`

Remove the `catch` clause the message names. When it is the only
clause and there is no `finally`, unwrap the `try` and keep its body.
For a multi-catch (`catch (A | B $e)`), remove only the type that is
never thrown.

**Stale detection:** the `catch` no longer names the type from the
message.

---

## Tier 4 — Unique to PHPantom

### H20. `generics.callSiteVarianceRedundant` — Remove redundant variance annotation

**Identifier:** `generics.callSiteVarianceRedundant`
**Tip (in message):** `You can safely remove the call-site variance annotation.`

Strip `covariant` or `contravariant` keywords from generic type arguments
in the docblock. Requires parsing PHPDoc generic syntax
(e.g. `Collection<covariant Foo>` becomes `Collection<Foo>`).
Also expose it through `phpantom_lsp fix --with-phpstan` as the
`phpstan.generics.callSiteVarianceRedundant` rule (see
[fix-cli.md](fix-cli.md#phpstan-integration)).

No other tool (PHPStorm, Rector, PHP-CS-Fixer) offers a quickfix for this
PHPStan-specific diagnostic. Users currently have to edit the PHPDoc manually
or suppress with `@phpstan-ignore`.

**Stale detection:** no `covariant`/`contravariant` in the PHPDoc on the
diagnostic line.

---

## Suggested implementation order

Based on effort-to-value ratio and shared infrastructure:

1. **H4** — unset by-ref foreach variable
2. **H13** — declare missing property
3. **H16** — add missing match arms
4. Everything else based on user demand

---

## Implementation notes

### Tip extraction

Tips are appended to `Diagnostic.message` after a `\n` by
`parse_phpstan_message()` in `phpstan.rs`. To access the tip:

```rust
let (message, tip) = match diag.message.split_once('\n') {
    Some((m, t)) => (m, Some(t)),
    None => (diag.message.as_str(), None),
};
```

Actions that depend on tip text (H4, H15, H20) should use this
pattern. The tip text has ANSI/HTML tags already stripped by `strip_ansi_tags`.

### Stale diagnostic detection

Each new action should have a corresponding check in
`is_stale_phpstan_diagnostic()` in `src/diagnostics/stale.rs` so that the
diagnostic is eagerly cleared after the user applies the fix, without
waiting for the next PHPStan run. New actions add another
`if identifier == "…"` branch there, next to the existing ones.

Identifiers whose fix is not visible from content (`throws.notThrowable`,
`missingType.checkedException`, `method.missingOverride`, …) are cleared
eagerly by `codeAction/resolve` instead.

### Testing

Each action needs tests following the existing pattern:
- Unit tests for pure helper functions (regex extraction, edit building)
- Integration tests that construct `CodeActionParams` with mock diagnostics
  and call `collect_*_actions` directly
- Stale detection tests that construct `Diagnostic` objects and call
  `is_stale_phpstan_diagnostic`

### Attribute insertion pattern

`find_method_insertion_point` is shared from `code_actions/phpstan/mod.rs`
and used by `add_override.rs` and `add_return_type_will_change.rs`. Future attribute-related actions should
reuse it.

### PHPDoc type mismatch pattern

`fix_phpdoc_type.rs` provides a shared helper parameterised by tag name
(`@return`, `@param`, `@var`). Each diagnostic offers two quickfixes:
update the tag type to match the native type, or remove the tag entirely
(preferred). Stale detection checks whether the tag still contains the
original PHPDoc type.

### Patterns from Rector

Several cross-cutting patterns from Rector's rule implementations are relevant
to all PHPStan code actions:

**Inheritance guard.** Before modifying a method's return type or parameter
type, check whether the method overrides a parent or interface method. Rector
uses `ClassMethodReturnTypeOverrideGuard` and
`ClassMethodReturnVendorLockResolver` for this. Modifying a type that is
constrained by a parent declaration would produce a fatal error. We already
have class hierarchy information available through `inheritance.rs`.

**Comment preservation.** When a code action inserts or removes lines near
existing comments or docblocks, take care not to orphan or lose them. Rector's
control-flow simplification rules merge comments from removed nodes onto the
first statement of the replacement.
