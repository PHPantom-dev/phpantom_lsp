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

### B550. A template's first import lands inside the block its first line opens

**Impact: Medium · Complexity: Medium**

```blade
@if ($user)
    {{ Carbon::now() }}
@endif
```

Completing `Carbon` writes `@use('Carbon\Carbon')` after the first line,
inside the `@if`. Blade compiles `@use` to a PHP `use` statement where it
stands, and PHP rejects one inside a block, so the view no longer compiles.
An import that sorts before every existing `@use` goes to the template's
start only when that start survives the trip through the source map
(`analyze_template_use_block`, `src/blade/use_block.rs`); a first line
that opens with `{{`, a directive, or a component tag does not, because a
Blade position at the start of a token maps to the end of the PHP it
lowers to, so the import moves after the line instead. The start of the
virtual line maps back to the template's start for such a line (it is
where the generated PHP begins), so the import can be planned there. A
template that opens with `@php` or `<?php` takes the import as a PHP `use`
just inside that block, and only a leading directive that lowers to
nothing at all (`@use`, `@inject`) leaves the end of the first line as the
place. BL1's "Import class" action writes its import through the same
code.

### B551. An unused `@use` import in a template is never reported

**Impact: Low · Complexity: Medium**

`collect_unused_import_diagnostics` reads the virtual PHP, where the
preprocessor hoisted each `@use` directive into the prologue as a real
`use` statement. The diagnostic's range lands in the prologue, which maps
to no template position, so it is dropped: a `use` inside `@php` is
reported, a `@use` directive never is. Report it at the directive's own
range (the scanner in `src/blade/use_directive.rs` knows where each one
is), and teach "Remove unused import" to delete the directive.

## Templates

No outstanding items.

## Miscellaneous

### B548. Moving a class rewrites the wrong `namespace` block when two blocks declare the same short name

**Impact: Low · Complexity: Low**

```php
<?php
namespace A { class Foo {} }
namespace B { class Foo {} }
```

Moving `B\Foo` to `C\Foo` rewrites `namespace A`. The move finds the class
being moved as the first `ClassDeclaration` span with its short name
(`src/rename/class/mod.rs`) and then takes the namespace declaration before
that span, without checking that the class it found is the one in the
namespace being moved. Match the declaration inside the old namespace's
block.

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
