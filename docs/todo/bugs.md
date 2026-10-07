# PHPantom — Bug Fixes

Every bug below must be fixed at its root cause. "Detect the
symptom and suppress the diagnostic" is not an acceptable fix.
If the type resolution pipeline produces wrong data, fix the
pipeline so it produces correct data. Downstream consumers
(diagnostics, hover, completion, definition) should never need
to second-guess upstream output.

Each entry below carries an **Impact · Complexity** rating using the same
scale defined in [`docs/todo.md`](../todo.md), but a bug's row lives
**here only** — do not add or link a bug entry to `docs/todo.md`'s sprint
or backlog tables. This file is its own list, not a domain document
sprint items draw from: whenever it holds anything, that is actively
addressed, independently of sprint planning.

Bugs land here from wherever they surface: found while working on another
task, or sweeps of the sample projects under `projects/`. Entries are
grouped by the mechanism that has to change, not by the symptom that
surfaced: one entry is one root cause, however many shapes it shows up in.

## Crashes

No outstanding items.

## Type comparison

No outstanding items.

## Standard-library return types

No outstanding items.

## Reachability

No outstanding items.

## Narrowing

### B550. Type-check functions only narrow when written in lowercase

**Impact: Low · Complexity: Low**

PHP function names are case-insensitive, so `Is_String($x)` and
`IS_RESOURCE($this->stream)` are the same checks as their lowercase
spellings. The guard table (`type_guard_kind_from_name` in
`type_engine/types/narrowing/guards.rs`), `narrows_first_argument`, and
the class-string and member-existence extractors match the name
case-sensitively, so a mixed-case guard narrows nothing and the branch
keeps the wide type. The `is_a()` and `in_array()` extractors already
compare case-insensitively. Fold the name to lowercase once where these
helpers read it (without allocating on the common lowercase path).

### B555. Comparing an array to an array literal with `===` narrows neither branch

**Impact: Low · Complexity: Medium**

```php
/** @param list{string} $arr */
function takesStringList(array $arr): void {}

function caller(?string $val): void
{
    $arr = [$val];           // array{string|null}
    if ($arr === [null]) {
        return;              // $arr is still array{string|null} here, not array{null}
    }
    takesStringList($arr);   // reported, but $arr can only be array{string} here
}
```

`literal_comparand_type`
(`src/type_engine/variable/forward_walk/cond_narrowing/predicates.rs`)
recognises only the empty array among array literals, so
`apply_literal_identity_narrowing` (`cond_narrowing/emptiness.rs`) never
sees `[null]` as a value to pin or strip, and `$arr` keeps its type in both
branches. Guarding on the element instead (`$arr[0] === null`) narrows
correctly. The equal branch should narrow the subject to the literal's
shape (`array{null}`). The unequal branch can subtract the literal from a
shape with the same keys when every other entry is already pinned to the
literal's value, which a one-entry shape always is.

The guarded call became a false positive in 0.11.0, which started
reporting the unguarded one (`array{string|null}` passed to
`list{string}`). PHPStan and Psalm report it too. mago narrows.

Found running the php-typing-conformance suite
(`regressions_array_element_null_subtraction.php`).

### B559. A `match (true)` arm with several conditions is narrowed as if all of them held

**Impact: Medium · Complexity: Low**

```php
function f(Cat|Dog|null $p): void {
    match (true) {
        // Runs when *either* check holds, so `$p` is `Cat|Dog` here,
        // but no mismatch is reported.
        $p instanceof Cat, $p instanceof Dog => takesDog($p),
        default => null,
    };
}
```

The arm body runs when any one of its conditions is `true`, but both the
diagnostic snapshot walker (`record_match_ternary_snapshots`) and the
completion/hover walker (`apply_cursor_ternary_narrowing`) apply each
condition's truthy narrowing to the same scope in turn, which is the
narrowing of `a && b`. Each condition should narrow its own copy of the
scope and the copies should be joined, as the `||` pass does.

### B560. An `&&` inside a `match (true)` arm condition does not narrow its later operands

**Impact: Medium · Complexity: Low**

```php
function f(?string $s): void {
    match (true) {
        // `strlen($s)` reports `?string`.
        is_string($s) && strlen($s) > 1 => null,
        default => null,
    };
}
```

The same condition narrows its right operand in an `if`, an assignment,
or a ternary, and an `&&` chain in an arm *body* narrows too. Only the arm
conditions of a `match (true)` are missing from the short-circuit snapshot
recording.

### B561. A `match (true)` passed straight into a call ignores its arm narrowing

**Impact: Medium · Complexity: Medium**

```php
function f(Cat|Dog $p): void {
    // Reports `Dog|Cat`; assigned to a variable first, the value is `Dog`.
    takesDog(match (true) { $p instanceof Cat => new Dog(), default => $p });
}
```

The `match` value's arm narrowing lives in the `Expression::Match` case of
`resolve_rhs_expression` (`rhs_resolution/mod.rs`), and none of it reaches
an argument, not even the `instanceof` extractor that works without a
scope. The same ternary passed as an argument does narrow, so the argument
path resolves a `match` through some other route than the ternary's; find
it and route it through the shared one.

## Arithmetic

No outstanding items.

## Symbol resolution

No outstanding items.

## Array types

### B553. An unsealed array shape is checked as the plain array it widens to

**Impact: Low-Medium · Complexity: Medium**

```php
/** @param array{foo: int, ...} $shape */
function take(array $shape): void {}

take(['buz' => 42.0]);   // not reported: `foo` is missing
take(['foo' => 'one']);  // not reported: `foo` is an int

/** @param list{string, int, ...} $values */
function takeList(array $values): void {}

takeList([1, 'demo', true]); // not reported: the first two entries are the wrong types
```

`kind()` reads a `TypeKind::UnsealedShape` as the generic array it widens
to, `non-empty-array<array-key, mixed>` for `array{foo: int, ...}`. The
shape rules of both type comparisons match on `kind()`: the
shape-to-shape rule in `is_subtype_of` (`src/php_type/subtype.rs`) and in
`is_type_compatible` (`src/diagnostics/type_errors/compatibility.rs`), and
`missing_required_shape_keys`, which names the missing key in the message.
None of them sees the listed entries, so an unsealed shape on either side
is held to its widened form alone. As a parameter or `@return` it accepts
any non-empty array (`[]` is still reported, but without naming `foo`). As
an argument it reaches a sealed `array{foo: string}` parameter as an array
of unknown keys, which the shape rules accept as a maybe. The sealed
spellings are unaffected.

This is a regression: 0.10.0 dropped the `...` and checked the shape as
sealed, so the missing key was reported.

**Fix:** Read an unsealed side through `as_unsealed_shape()` in those three
places. Compare its listed entries the way two shapes are compared (a
required key is present, a key both sides name holds a value that fits),
then hold every other entry of the narrower side to the tail's key and
value types. An unsealed `list{…, ...}` also keeps the list rules for its
positional entries.

Found running the php-typing-conformance suite (`arrays_open_shapes.php`,
`arrays_unsealed_shape.php`, `arrays_unsealed_shape_optional_key.php`).

## Laravel

No outstanding items.

## Blade

No outstanding items.

## Templates

No outstanding items.

## Miscellaneous

### B549. `@throws` and namespaced-function completions plan their import against the whole file

**Impact: Low · Complexity: Low-Medium**

The `@throws` smart items (`src/completion/phpdoc/mod.rs`), the `@throws`
imports of docblock generation (`build_throws_import_edits` in
`src/completion/phpdoc/generation/mod.rs`) and `build_function_completions`
(`src/completion/context/function_completion.rs`) build their `use` edit
from `analyze_use_block(content)`, the whole file. In a file with several
`namespace` blocks the import can land in another block, which every other
import edit stopped doing this cycle; in a Blade template it lands in the
virtual prologue, so the completion carrying it is dropped. Plan them
through `Backend::use_block_for` with the block the cursor is in, as class
completion does.

### B554. `->value` and `->name` on an enum read as `string`, not as its cases' values

**Impact: Medium · Complexity: Low-Medium**

```php
enum Suit: string { case Hearts = 'hearts'; case Spades = 'spades'; }

/** @param value-of<Suit> $value */
function take(string $value): void {}

take($suit->value);  // reported: expects 'hearts'|'spades', got string

final class Card
{
    public function __construct(private Suit $suit) {}

    /** @return value-of<Suit> */
    public function suitValue(): string
    {
        return $this->suit->value; // reported: string is incompatible with 'hearts'|'spades'
    }
}
```

The inheritance merge (`src/inheritance/mod.rs`, where it refines a backed
enum's `value` property) narrows `BackedEnum::$value` from `int|string` to
the enum's backing type and stops there, and `UnitEnum::$name` stays
`string`. A named case already reads as its own literal
(`Suit::Hearts->value` is `'hearts'`), but a value typed as the enum reads
as the bare scalar. This became a false positive in 0.11.0, when
`value-of<…>` over an enum started evaluating to the cases' values instead
of staying unevaluated (which accepted anything): every `value-of<Enum>`
parameter or return fed an enum's `->value` is now reported. So is a
declared literal union (`@return 'hearts'|'spades'`), and a `@template T of
Suit` function returning `$case->value` as `value-of<T>`.

**Fix:** Refine `value` to the union of every case's backing value, and
`name` to the union of the case names, as PHPStan and Psalm do. When a
case's value cannot be read (a constant expression the folder does not
handle), keep the backing type.

Found running the php-typing-conformance suite
(`phpdoc_advanced_fallback_value_of_template_enum.php`).

### B556. Moving a class out of a braced global `namespace { }` block writes an unbracketed `namespace`

**Impact: Low · Complexity: Low-Medium**

```php
<?php
namespace { class Foo {} }
```

Moving `Foo` to `C\Foo` inserts `namespace C;` above the block, and PHP
refuses a file that mixes bracketed and unbracketed `namespace`
declarations. A braced global block has no name for
`namespace_declaration_edits` (`src/rename/class/mod.rs`) to rewrite, so the
move takes the path for a file that had no `namespace` at all. Write the new
name after the block's `namespace` keyword instead, and place the imports the
former global siblings now need in that block.

### B558. Removing two unused members at the end of a group import breaks the statement

**Impact: Medium · Complexity: Low-Medium**

```php
use App\Models\{User, Post, Comment};   // only User is used
```

"Remove all unused imports" and `phpantom_lsp fix` turn this into
`use App\Models\{User, `, dropping the closing `};`. Each member is removed
on its own by `extend_range_for_group_member`
(`src/code_actions/remove_unused_import.rs`): a member takes the comma after
it, or the one before it when it is the last, so `Post` takes `Post, ` and
`Comment` takes `, Comment`. The two edits overlap, and `apply_text_edits`
applies the second against text the first already changed. A member has to
choose its comma knowing the rest of the batch: the one after it while every
member before it is removed too, the one before it otherwise. That way no
two removals share a comma. The removal of a template's `@use` group
members (`group_member_removal`, in the same file) already chooses this way.

### B559. An import written into a `namespace` block that sits on one line lands after the block

**Impact: Low · Complexity: Medium**

```php
<?php
namespace B { class Foo { public function f(): Helper {} } }
```

With `B\Helper` declared elsewhere, moving `B\Foo` to `C\Foo` writes
`use B\Helper;` on the line below the block, outside every `namespace`, and
PHP refuses the file. `analyze_use_block_in` (`src/completion/use_edit.rs`)
puts the first import of a block that has none on the line after its
`namespace` line, which is past the block when the block closes on that
line, and every import planned through it shares the placement. The import
belongs just after the `{` or `;` of the declaration, which `UseBlockInfo`
cannot express: its positions are whole lines.

### B560. Moving a class out of one section of a file with several unbraced `namespace` statements into the global namespace changes its namespace

**Impact: Low · Complexity: Low**

```php
<?php
namespace A;

class Foo {}

namespace B;

class Bar {}
```

Moving `B\Bar` to `Bar` deletes the `namespace B;` line, and `Bar` is then
declared in `A`, while every reference was rewritten to name the global
class. Moving `A\Foo` instead leaves global code ahead of `namespace B;`,
which PHP refuses. `remove_namespace_edits` (`src/rename/class/layout.rs`)
treats an unbraced statement as removable whatever follows it, but in a file
with several sections it is what separates them. Refuse the move the way a
brace-style `namespace` is refused, since the sections would have to be
rewritten as braced blocks.
