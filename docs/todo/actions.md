# PHPantom — Code Actions

Items are ordered by **impact** (descending), then **effort** (ascending)
within the same impact tier.

| Label      | Scale                                                                                                                  |
| ---------- | ---------------------------------------------------------------------------------------------------------------------- |
| **Impact** | **Critical**, **High**, **Medium-High**, **Medium**, **Low-Medium**, **Low**                                           |
| **Effort** | **Low** (≤ 1 day), **Medium** (2-5 days), **Medium-High** (1-2 weeks), **High** (2-4 weeks), **Very High** (> 1 month) |

**Refactoring code actions overview:** A2 (Extract Function) depends on
forward-pass variable usage tracking with byte offsets across function
scopes.

## A34. Unified code action handler architecture

**Impact: Medium · Effort: Medium-High**

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

---



---

## A16. Snippet Placeholder for Extracted Method Name

**Impact: Medium · Effort: Low-Medium**

> **Blocked:** Requires `SnippetTextEdit` support in `lsp-types`.
> Upstream issue: [gluon-lang/lsp-types#310](https://github.com/gluon-lang/lsp-types/issues/310).
> The current `lsp-types` (0.94, pinned by `tower-lsp` 0.20) only
> covers LSP 3.17. `SnippetTextEdit` is an LSP 3.18 proposed feature.
> Revisit once the upstream issue is resolved and `tower-lsp` picks up
> the new version.

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

**Impact: Medium · Effort: Low**

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

**Impact: Medium · Effort: Low**

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

**Impact: Low-Medium · Effort: Medium**

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

**Impact: Low-Medium · Effort: Medium**

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

**Impact: Low-Medium · Effort: Medium**

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

**Impact: Low-Medium · Effort: Medium**

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

### A39. Convert to string interpolation

**Impact: Low-Medium · Effort: Low**

Replace simple string concatenation with double-quoted string
interpolation:

```php
// Before
$greeting = 'Hello ' . $name . ', welcome!';
$msg = "Total: " . $order->getTotal();

// After
$greeting = "Hello {$name}, welcome!";
$msg = "Total: {$order->getTotal()}";
```

#### When the conversion is safe

- The concatenation contains at least one variable or simple
  expression (`$var`, `$var->prop`, `$arr['key']`) and at least one
  string literal.
- No interpolated part contains characters that would need escaping
  in a double-quoted string (`$`, `"`, `\`) beyond what is already
  escaped, unless the tool handles the escaping.
- Existing single-quoted string literals in the concatenation are
  re-quoted as double-quoted, with `$` and `"` characters escaped.
- Method calls like `$obj->method()` require curly-brace syntax
  (`{$obj->method()}`), which is valid in PHP.
- Integer, float, and boolean literals are left as concatenation
  (they don't benefit from interpolation and `true`/`false` would
  print as `1`/empty string).
- The concatenation must be a top-level expression or RHS of an
  assignment, not nested inside a function call argument where
  readability is subjective.

#### Implementation

- Walk the AST for `Expression::Concat` (binary `.` operator) nodes.
- Collect the flattened chain of concat operands (recursively unwrap
  nested concats).
- If the chain is all literals or all variables (no mix), skip.
- Build a double-quoted string: literal parts are inserted verbatim
  (with `$` and `"` escaped), variable/expression parts are wrapped
  in `{...}`.
- Replace the entire concat expression with the interpolated string.

**Code action kind:** `refactor.rewrite`.

---

### A40. Generate method from call

**Impact: High · Effort: Medium**

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

**Impact: High · Effort: Medium**

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

**Impact: Medium · Effort: Medium**

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
