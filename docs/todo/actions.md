# PHPantom — Code Actions

Items are ordered by **impact** (descending), then **complexity** (ascending)
within the same impact tier.

| Label      | Scale                                                                                                                  |
| ---------- | ---------------------------------------------------------------------------------------------------------------------- |
| **Impact** | **Critical**, **High**, **Medium-High**, **Medium**, **Low-Medium**, **Low**                                           |
| **Complexity** | **Low** (mechanical/boilerplate, no design decisions), **Medium** (self-contained, follows an existing pattern), **Medium-High** (spans modules, some new design), **High** (shared/core subsystem, correctness or performance tradeoffs), **Very High** (cross-cutting architecture, wide blast radius) |

## A16. Snippet Placeholder for Extracted Method Name

**Impact: Medium · Complexity: Medium**

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
   save the snippet edit flag on the `Backend` struct, the same way
   `supports_code_action_resolve` is stored.

2. **Check the flag in `resolve_extract_function`** (extract function
   is a deferred action, so its edit is built at resolve time).  When
   the client supports snippet edits, build the workspace edit's
   `TextDocumentEdit`s with `Edit::SnippetTextEdit` entries instead of
   `Edit::TextEdit`.  The new-text for the method name uses
   `${1:name}` syntax.

3. **Linked edit ranges (optional enhancement).**  If the client
   supports `workspace.workspaceEdit.changeAnnotationSupport` or
   linked edit groups, use those so that editing the name at the
   definition also updates the call site in real time.

### Prerequisites

| Feature                          | What it contributes                                       |
| -------------------------------- | --------------------------------------------------------- |
| Client capability storage        | Need to know whether the client supports snippet edits    |
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
operator. The ternary form (`$x !== null ? $x->foo() : null` →
`$x?->foo()`) already ships in `src/code_actions/simplify_null.rs`; what
remains is the if-statement form, which belongs in the same module:

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
- The null check is `$var !== null`, `$var != null`, `!is_null($var)`,
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

### A38. Convert if/elseif chain to `match`

**Impact: Low-Medium · Complexity: Medium-High**

Convert an if/elseif chain that compares the same subject against
different values into a `match` expression:

```php
// Before
if ($status === 'active') {
    $label = 'Active';
} elseif ($status === 'inactive' || $status === 'disabled') {
    $label = 'Off';
} else {
    $label = 'Unknown';
}

// After
$label = match ($status) {
    'active' => 'Active',
    'inactive', 'disabled' => 'Off',
    default => 'Unknown',
};
```

`match` compares with `===`, so a chain of strict comparisons converts
exactly, which `switch`'s loose comparison could not offer. It also
sits next to the shipped switch → match action
(`convert_switch_to_match.rs`) rather than working against it.

#### When the conversion is safe

- Every condition compares the same subject expression against a
  value with `===`, optionally several of them joined by `||` (which
  become one arm with comma-separated conditions). A `==` chain is
  only converted when the subject's type makes loose and strict
  comparison agree (e.g. both sides are strings).
- The subject is side-effect free (variable, property access), since
  it is evaluated once instead of once per condition.
- `match` arms are expressions, so every branch must reduce to the same
  shape: a single `return <expr>;` in each branch (the result is
  `return match (…) { … };`), a single assignment to the same variable
  in each branch (`$x = match (…) { … };`), or a `throw` (a valid arm
  expression since PHP 8.0) in any branch of either form. Branches
  with more than one statement are not converted.
- The chain has a trailing `else`, which becomes `default`. Without
  one, `match` throws `UnhandledMatchError` where the if-chain did
  nothing, so the action is not offered. (A `return` chain followed
  directly by a `return` statement can use that statement as the
  `default` arm.)

#### Implementation

- Walk the AST for `Statement::If` nodes with at least one `elseif`.
- Extract the subject from the first condition and verify every other
  condition compares the same subject (by AST structure equality).
- Check the branch shape above, build the `match`, and replace the
  whole if/elseif/else block (plus a following `return` when it was
  folded into `default`).

**Code action kind:** `refactor.rewrite`.
**Guard:** `php_version >= 8.0`.

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
this as a transformer. Typing `/**` above a class already generates
these tags for a new docblock
(`src/completion/phpdoc/generation/build.rs`); what remains is a code
action for a class that already has a docblock, naturally alongside
`update_docblock.rs`.

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

---

### A46. Convert nested calls to the pipe operator (PHP 8.5+)

**Impact: Low-Medium · Complexity: Medium**

```php
$slug = strtolower(trim(str_replace(' ', '-', $title)));
// →
$slug = $title
    |> (fn($x) => str_replace(' ', '-', $x))
    |> trim(...)
    |> strtolower(...);
```

PhpStorm offers this on any call nested in another call's sole or first
argument. A call whose only argument is the inner result becomes a
first-class callable (`trim(...)`); one with other arguments becomes an
arrow function, since `|>` passes exactly one argument. Offer it only when
the project's PHP version is 8.5 or later, and the reverse ("Inline pipe
into nested calls") on a `|>` chain.

---

### A47. Convert a getter or setter method into a property hook (PHP 8.4+)

**Impact: Low · Complexity: Medium**

```php
private string $name;
public function getName(): string { return strtoupper($this->name); }
// →
public string $name { get => strtoupper($this->name); }
```

"Generate property hooks" (`src/code_actions/generate_property_hooks.rs`)
adds an empty hook to a property. PhpStorm also offers the migration: when
a method only returns (or only assigns) one property, move its body into
the property's hook, make the property public, and rewrite the callers
(`$x->getName()` → `$x->name`) through the rename machinery. Skip it when
the method is part of an interface or overrides a parent, since the
callers through that contract cannot be rewritten.
