# PHPantom — Code Actions

Items are ordered by **impact** (descending), then **complexity** (ascending)
within the same impact tier.

| Label      | Scale                                                                                                                  |
| ---------- | ---------------------------------------------------------------------------------------------------------------------- |
| **Impact** | **Critical**, **High**, **Medium-High**, **Medium**, **Low-Medium**, **Low**                                           |
| **Complexity** | **Low** (mechanical/boilerplate, no design decisions), **Medium** (self-contained, follows an existing pattern), **Medium-High** (spans modules, some new design), **High** (shared/core subsystem, correctness or performance tradeoffs), **Very High** (cross-cutting architecture, wide blast radius) |

**Refactoring code actions overview:** A2 (Extract Function) depends on
forward-pass variable usage tracking with byte offsets across function
scopes.

## A34. Unified code action handler architecture

**Impact: Medium · Complexity: Very High**

Refactor the code action system to use a unified handler architecture
inspired by rust-analyzer's assist system. Currently each code action
has a separate `collect_*` method called from a hand-maintained list in
`handle_code_action`, and deferred actions have a separate `resolve_*`
method dispatched via a string match in `resolve_code_action`. PHPStan
quick-fixes and refactoring actions use different code paths.

### Changes

1. **Unified handler signature.** Each code action becomes a function
   `fn(&mut Actions, &ActionContext) -> Option<()>`. Handlers are
   collected in a static array. `handle_code_action` iterates the array
   instead of calling methods one by one.

2. **Closure-based lazy resolve.** Handlers call
   `actions.add(id, label, range, |builder| { ... })`. The closure
   only runs when the action is being resolved, eliminating separate
   `collect_*` / `resolve_*` method pairs. The same handler function
   serves both Phase 1 (applicability check + lightweight stub) and
   Phase 2 (compute edit).

3. **Unified type for actions and diagnostic fixes.** Use the same
   struct for PHPStan quick-fixes and refactoring actions. The LSP
   layer gets one conversion path. Diagnostic fixes attach the same
   type as their quick-fix data.

4. **Sort by target range size.** Sort results by `target.len()` as
   a tiebreaker. Smaller target = more specific = higher priority.
   No manual priority numbers needed.

### When to implement

Do this when the next batch of code actions is added (A25, A28, etc.).
The refactoring pays for itself by making each subsequent action
cheaper to add: write one function, append it to an array.

---

## A46. Honor `context.only` in code action responses

**Impact: Medium · Complexity: Medium**

We advertise `codeActionKinds` (`quickfix`, `refactor.extract`,
`refactor.inline`, `source.organizeImports`) in `code_action_provider`
(`src/server.rs`), and every collector already tags its `CodeAction`
with a `kind`, but `handle_code_action` in `src/code_actions/mod.rs`
never reads `params.context.only` and never filters its result against
it. A client that asks for only quick-fixes (its Ctrl+.-style menu) or
only `source.organizeImports` (an on-save automation like
`editor.codeActionsOnSave`) gets every action mixed together instead,
which breaks the menus that are supposed to be filtered and can make
an auto-run-on-save hook unable to identify the one action it should
apply.

### The kind/position interaction to get right

`context.only` is a pure **kind** filter and must stay orthogonal to
the **position/selection** validity checks each collector already does
(a diagnostic overlapping `params.range` for quickfixes, a real
non-collapsed selection over a literal for `extract_constant`, a
cursor on an inlineable variable for `inline_variable`, etc.). Two
failure modes to design against explicitly:

1. **Don't let a kind mismatch masquerade as a position mismatch, or
   vice versa.** Implement the kind check as a separate, earlier gate
   than the position check, never folded into the same conditional —
   e.g. `if kind_requested(&only, KIND) { collect_x(...) }`, where
   `collect_x` still runs its own unmodified position logic once
   called. If the two checks get combined, a future edit to one is
   liable to silently change the other's behavior.
2. **Filter before running a collector, not just after, where cheap to
   do so.** Every collector's kind is fixed and known statically (see
   the table below), so `context.only` can skip whole collector calls
   before they touch the AST, rather than computing every action and
   discarding some after the fact. This isn't just an optimization —
   `handle_code_action` runs on effectively every cursor move, so a
   client that always sends a narrow `only` (several editors default
   the automatic lightbulb request to `quickfix` only) currently pays
   for every refactor/extract collector on every keystroke for
   nothing.

`only` matching must be **hierarchical**, not exact-string: a client
requesting the bare `"refactor"` kind must match any of our
`refactor.extract`/`refactor.inline`/`refactor.rewrite` actions
(`requested == actual || actual.starts_with(&format!("{requested}."))`),
per the LSP spec's kind-hierarchy semantics — an exact-equality check
would silently return nothing for that request.

### A second gap this surfaces

Several collectors already emit kinds we don't declare in
`code_action_provider`'s `code_action_kinds` list: `refactor.rewrite`
(`convert_switch_to_match`, `convert_to_arrow_function`,
`convert_to_closure`, `convert_to_interpolation`,
`promote_constructor_param`, `simplify_null`), `refactor.rewrite` via
`CodeActionKind::REFACTOR_REWRITE` (`generate_constructor`), and the
bare `CodeActionKind::REFACTOR` (`generate_getter_setter`,
`generate_property_hooks`, `replace_fqcn`). Reconcile these as part of
the same task: either add the missing kinds to the advertised list, or
fold each action under an already-declared parent kind — a client
that trusts our advertised capability list and asks for exactly what
we declared should not miss actions we're actually capable of
returning.

### Scope boundary: `codeAction/resolve` is unaffected

`context.only` belongs to the `textDocument/codeAction` request only.
`codeAction/resolve` (`resolve_code_action`) receives the exact
`CodeAction` the client already picked (via its `data` field) with no
`context` to filter against — do not add any kind-filtering logic
there.

**Where to look:** `handle_code_action` in `src/code_actions/mod.rs`
(the ~30 `collect_*_actions` call list — each call site is where the
early kind-gate belongs); `code_action_provider` in `src/server.rs`
for the advertised kind list; A34 above is the natural long-term home
for a per-handler kind table if that refactor lands first, but this
doesn't need to wait for it — the gate works fine bolted onto the
current call list.

---



---

## A16. Snippet Placeholder for Extracted Method Name

**Impact: Medium · Complexity: Medium**

> **Blocked:** Requires `SnippetTextEdit` support in `lsp-types`.
> Upstream issue: [gluon-lang/lsp-types#310](https://github.com/gluon-lang/lsp-types/issues/310)
> (open). `SnippetTextEdit` is an LSP 3.18 `@proposed` feature — it's
> in the spec but not yet implemented in any Rust LSP-types crate,
> including the actively maintained `tower-lsp-server`/`ls-types` fork
> that F20 migrates to (checked directly against its `main` branch;
> `ls-types` removed its generic 3.18 `"proposed"` feature flag in
> 0.0.4). F20 alone does not resolve this. Revisit once the upstream
> issue is resolved in whichever crate we depend on at the time.

After an Extract Function/Method code action is applied, let the user
immediately rename the generated name by placing a snippet tab-stop on
it.  The contextual name (`createUsers`, `validateGuard`, …) serves as
the default, but the cursor lands directly on it so the user can type
over it without an extra rename step.

### Behaviour

- **Trigger:** User applies "Extract method 'createUsers'" (or any
  extract function/method action).
- **Result:** The workspace edit uses a `SnippetTextEdit` with
  `${1:createUsers}` for the method name at both the definition site
  and every call site.  The editor enters snippet mode and the user
  can type a new name that updates all locations simultaneously.
- **Fallback:** When the client does not advertise
  `workspace.workspaceEdit.snippetEditSupport`, emit a regular
  `TextEdit` (current behaviour — no snippet, no cursor placement).

### Implementation

1. **Store client capabilities at initialisation.**  In `initialize`,
   save the `InitializeParams.capabilities` (or at least the snippet
   edit flag) on the `Backend` struct.

2. **Check the flag in `collect_extract_function_actions`.**  When
   the client supports snippet edits, build the workspace edit with
   `DocumentChanges::Operations` containing `SnippetTextEdit` entries
   instead of plain `TextEdit`.  The new-text for the method name
   uses `${1:name}` syntax.

3. **Linked edit ranges (optional enhancement).**  If the client
   supports `workspace.workspaceEdit.changeAnnotationSupport` or
   linked edit groups, use those so that editing the name at the
   definition also updates the call site in real time.

### Prerequisites

| Feature                          | What it contributes                                       |
| -------------------------------- | --------------------------------------------------------- |
| Client capability storage        | Need to know whether the client supports snippet edits    |
| `SnippetTextEdit` in tower-lsp   | Verify tower-lsp exposes the snippet edit type            |
| Extract Function (shipped)       | The code action that this enhances                        |

---

## IDE-expected code actions

The following actions are offered by competing PHP IDEs (PHPStorm,
Intelephense) and are expected by users. Identified by cross-referencing
Rector, PHP-CS-Fixer, and Phpactor rule libraries against what major
IDEs actually surface as on-demand code actions.

Micro-simplifications (array_push→$arr[], strlen→==='', flip ternary,
etc.) are intentionally excluded. They are better served by batch tools
like Rector or PHP-CS-Fixer. An LSP should focus on actions that
benefit from editor context (cursor position, file state) rather than
competing with CLI transformers.

---

### A25. `strpos` → `str_contains` (PHP 8.0+)

**Impact: Medium · Complexity: Medium**

Convert `strpos($haystack, $needle) !== false` to
`str_contains($haystack, $needle)` and the negated form
`strpos($haystack, $needle) === false` to
`!str_contains($haystack, $needle)`.

Also handle `strstr($haystack, $needle) !== false`.

PHPStorm offers this as an inspection with quick-fix. PHP-CS-Fixer's
`ModernizeStrposFixer` is the reference implementation. Edge case:
must verify exactly 2 arguments to `strpos` (the 3-argument form with
offset has different semantics).

**Code action kind:** `refactor.rewrite`.
**Guard:** `php_version >= 8.0`.

---

### A28. Explicit nullable parameter type (PHP 8.4 deprecation)

**Impact: Medium · Complexity: Low**

Convert implicit nullable parameters to explicit nullable syntax:
`function foo(string $p = null)` → `function foo(?string $p = null)`.

PHP 8.4 deprecates the implicit nullable form. PHPStorm flags this.
PHP-CS-Fixer's `NullableTypeDeclarationForDefaultNullValueFixer`
handles union types, intersection types (DNF), and constructor
property promotion.

Only offer when the parameter has a type hint, a `= null` default, and
the type does not already include `null` (no `?` prefix, no `|null`
in a union).

**Code action kind:** `quickfix`.

---

### A29. Simplify boolean return

**Impact: Low-Medium · Complexity: Medium**

Convert if-return-boolean patterns to direct boolean returns:

- `if ($a === $b) { return true; } return false;` → `return $a === $b;`
- `if ($a === $b) { return false; } return true;` → `return $a !== $b;`

PHPStorm offers this. When the condition is not already boolean-typed,
wrap with `(bool)`.

Guard conditions:
- The if must have exactly one statement (a return of `true` or `false`)
  and no else/elseif.
- The next sibling statement must be `return` of the opposite boolean.

**Code action kind:** `refactor.rewrite`.

---

### A31. Remove always-else (extract guard clause)

**Impact: Low-Medium · Complexity: Medium-High**

When an if-body ends with a flow-breaking statement (`return`, `throw`,
`continue`, `exit`), the `else` keyword is redundant. Promote the else
body to the same nesting level.

PHPStorm marks this as "unnecessary else". PHP-CS-Fixer's
`NoUselessElseFixer` is the reference. Edge case: don't remove else
blocks containing named function or class declarations (PHP evaluates
these eagerly, removing the else changes semantics).

**Code action kind:** `refactor.rewrite`.

---

### A37. Simplify with `?->` (nullsafe operator)

**Impact: Low-Medium · Complexity: Medium-High**

Replace null-checked method/property chains with PHP 8.0's nullsafe
operator:

```php
// Before
if ($user !== null) {
    $name = $user->getName();
}

$city = null;
if ($user !== null) {
    $city = $user->getAddress()->getCity();
}

// After
$name = $user?->getName();

$city = $user?->getAddress()?->getCity();
```

#### When the conversion is safe

- The if-body contains exactly one statement: an assignment or a
  standalone expression statement using the checked variable.
- The null check is `$var !== null`, `$var !== null`, `!is_null($var)`,
  or `isset($var)` (for a single variable, not array access).
- There is no `else` / `elseif` branch. An else branch means the
  developer wants to handle the null case explicitly, which `?->`
  cannot express.
- The variable is used only with `->` access in the body (not passed
  to a function, not reassigned, not used in a binary expression).
- For chained access (`$a->b()->c()`), every intermediate `->` must
  also be converted to `?->` because the nullsafe operator
  short-circuits the entire chain.
- If the body assigns to a variable (`$x = $var->foo()`), the
  resulting `$x = $var?->foo()` produces `null` when `$var` is null,
  which matches the pre-existing state (the assignment was skipped
  entirely, so `$x` was either unset or previously null).

#### Implementation

- Walk the AST for `Statement::If` nodes where the condition is a
  null check on a single variable.
- Verify the body meets the safety criteria above.
- Replace the entire if-block with the body statement, substituting
  every `->` on the checked variable's access chain with `?->`.
- When the if-block only contains a standalone expression (no
  assignment), emit just the expression with `?->`.

**Code action kind:** `refactor.rewrite`.
**Guard:** `php_version >= 8.0`.

---

### A38. Convert if/elseif chain to switch

**Impact: Low-Medium · Complexity: Medium-High**

Convert an if/elseif chain that compares the same variable or
expression against different values into a `switch` statement:

```php
// Before
if ($status === 'active') {
    doActive();
} elseif ($status === 'inactive') {
    doInactive();
} elseif ($status === 'pending') {
    doPending();
} else {
    doDefault();
}

// After
switch ($status) {
    case 'active':
        doActive();
        break;
    case 'inactive':
        doInactive();
        break;
    case 'pending':
        doPending();
        break;
    default:
        doDefault();
        break;
}
```

#### When the conversion is safe

- Every condition in the chain compares the same subject expression
  against a constant value using `===` or `==` (all arms must use the
  same comparison operator).
- The subject expression is a simple expression (variable, property
  access, method call) that should not have side effects when evaluated
  once in the switch head instead of repeatedly in each condition.
- With `===`, the conversion is semantically exact only for scalar
  values. Switch uses loose comparison internally, so strict-equality
  chains are converted with a comment noting the semantic difference,
  or the action is only offered for `==` chains.

#### Implementation

- Walk the AST for `Statement::If` nodes that have at least one
  `elseif` branch.
- Extract the subject from the first condition's comparison. Verify
  all subsequent conditions compare the same subject (by source text
  or AST structure equality).
- Build a `switch` statement: each condition value becomes a `case`,
  the if/elseif body becomes the case body with `break;` appended
  (unless the body ends with `return`, `throw`, or `continue`).
- If the chain has a trailing `else`, convert it to `default:`.
- Replace the entire if/elseif/else block with the switch.

**Code action kind:** `refactor.rewrite`.

---

### A40. Generate method from call

**Impact: High · Complexity: Medium-High**

When invoking an undefined method (e.g. `$foo->newMethod($a, $b)`),
offer a code action to generate a method stub on the target class
with the correct signature inferred from the call-site arguments.
High-impact rapid-prototyping workflow. Phpactor has this.

- Resolve the type of the subject to find the target class and file.
- Infer parameter types from the argument expressions at the call
  site (literal types, variable types, class hints).
- Infer return type as `void` by default; if the call is used in an
  assignment or return context, use `mixed`.
- Insert the generated method at the end of the class body (before
  the closing `}`).
- Visibility defaults to `public`; offer a choice if the call is
  within the same class (`private`/`protected`).

**Code action kind:** `quickfix`.
**Trigger:** Unknown-member diagnostic on a method call.

---

### A41. Create class from non-existing name

**Impact: High · Complexity: Medium-High**

When a class name cannot be resolved, offer a code action to
generate a new class file with the correct namespace based on PSR-4
mapping. Pairs naturally with the unknown-class diagnostic.
Phpactor has this.

- Use the PSR-4 autoload map from `composer.json` to determine the
  file path and namespace for the new class.
- Create the file with a minimal class skeleton (`<?php` declaration,
  `namespace`, empty class body).
- If the unresolved name is used in an `extends` or `implements`
  clause, generate the appropriate `class` or `interface` keyword.
- Add a `use` import at the call site if necessary.

**Code action kind:** `quickfix`.
**Trigger:** Unknown-class diagnostic.

---

### A43. Update docblock generics

**Impact: Medium · Complexity: Medium**

Auto-update or add `@extends`/`@implements` tags to match the actual
class hierarchy when a class extends a generic parent. Phpactor has
this as a transformer.

- Inspect the `extends` and `implements` clauses of the class under
  the cursor.
- For each parent/interface that declares `@template` parameters,
  check whether the current class has a matching `@extends` or
  `@implements` tag.
- If the tag is missing, generate one with placeholder type
  parameters (e.g. `@extends Collection<mixed>`).
- If the tag exists but the template parameter count has changed,
  update it to match.

**Code action kind:** `quickfix`.

---

### A45. Simplify with `?:` (Elvis operator)

**Impact: Low-Medium · Complexity: Medium**

Replace `$x ? $x : $y` with `$x ?: $y` (PHP's short ternary / "Elvis"
operator), the mirror image of the null-coalescing simplifications
`src/code_actions/simplify_null.rs` already offers. That module's
`try_simplify_ternary` currently handles `isset($x) ? $x : $default` and
`$x !== null ? $x : $default` (both → `??`) and the nullsafe patterns,
but not the plain case where the condition and the then-branch are
literally the same expression — its doc comment even notes short ternary
is only handled as something to *leave alone* on the input side, not
something to produce as output.

- Match a full ternary (`Conditional` with a `then` branch) whose
  condition's source text equals the then-branch's source text exactly,
  the same string-equality heuristic the existing `??` patterns use.
- Emit `<condition> ?: <else>`, dropping the duplicated then-branch.
- `??` already covers the null-specific version of this
  (`$x !== null ? $x : $default` → `$x ?? $default`); this pattern is
  for the truthiness version (`$x ? $x : $default` → `$x ?: $default`),
  which is a different operator with different semantics (falsy values
  other than `null`, like `0`, `''`, or `false`, also take the
  else-branch) and should not be conflated with it.
- Natural home is alongside the existing patterns in
  `try_simplify_ternary`, reusing its cursor-position walk and
  `Simplification` enum rather than a new module.

**Code action kind:** `refactor.rewrite`.
