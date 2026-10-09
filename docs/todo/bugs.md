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

### B568. A passing strict `in_array()` against `object` elements drops the needle's classes

**Impact: Low · Complexity: Low-Medium**

```php
/** @param list<object> $handlers */
function f(Foo|Bar|string $x, array $handlers): void {
    if (in_array($x, $handlers, true)) {
        $x->run(); // `$x` reads `string`, though the object it equals can be a `Foo` or a `Bar`
    }
}
```

In the branch where the check held, `apply_in_array_narrowing`
(`cond_narrowing/in_array.rs`) narrows the needle's class layer through
`apply_instanceof_inclusion`, which keeps only the classes the element type
resolves to. An element that names no class it can load, such as `object`
or a class that is not found, resolves to none, so every class goes and
only the needle's scalar alternatives are left. The branch already skips an
element that could be anything (`mixed`), but `object` can be any object as
well, and a class that cannot be loaded may be an ancestor of the needle's.

**Fix:** Narrow the class layer only when every element alternative that
can hold an object names a class that loads, and leave the needle's classes
alone otherwise.

## Arithmetic

No outstanding items.

## Symbol resolution

No outstanding items.

## Array types

### B565. A plain array counts as a subtype of an unsealed array shape

**Impact: Low · Complexity: Low-Medium**

```php
/** @param array{foo: int, ...} $shape */
function takeOpen(array $shape): void {}
```

`array<string, int>` is a subtype of `array{foo: int, ...}` as far as
`is_subtype_of_typed` is concerned, and `non-empty-array<string, int>` is
one for `PhpType::is_subtype_of` too, though nothing says either array
holds `foo`. The same goes for `list<int>` and `non-empty-list<int>`
against `list{int, ...}`. The shape-to-shape rules read an unsealed shape
through `shape_parts()`, but a subtype that is not itself a shape reaches
an unsealed supertype as the `non-empty-array<array-key, mixed>` it widens
to, which any array-like generic fits. The class-aware generic covariance
rule in `is_subtype_of_typed` does not even check the `non-empty-` promise,
which is why a bare `array<string, int>` passes there. A sealed supertype
has no such problem: `non-empty-array<string, int>` is not a subtype of
`array{foo: int}`. Parameter seeding reads the answer as a proven
narrowing of the declared type.

**Fix:** Settle an array-like generic against an unsealed supertype as an
unsealed shape with no entries of its own, `array{...<K, V>}` (or
`list{...<V>}` for a list), through `shape_is_subshape`: an entry the
supertype requires is then missing, and an optional one has to fit the
tail. Do it in `PhpType::is_subtype_of` and, ahead of the generic-array
rules, in `is_subtype_of_typed`. The argument diagnostic answers a typed
array handed to an unsealed parameter through its own rules, not through
these two functions; keep it that way, so that an array that merely might
hold the entries stays unreported.

### B566. A `list` parameter rejects a docblock shape keyed by class constants

**Impact: Low · Complexity: Low-Medium**

```php
class Slots { const NAME = 0; const AGE = 1; }

/** @return array{Slots::NAME: string, Slots::AGE: int} */
function row(): array { return ['Ann', 30]; }

/** @param list<string|int> $values */
function takeList(array $values): void {}

takeList(row()); // reported, though the keys are `0` and `1`
```

`shape_fits_array` (`src/diagnostics/type_errors/compatibility.rs`) holds
the keys of a shape to the integers a list demands by their spelling, and
a key spelled `Slots::NAME` is not one, so the shape is rejected whatever
the constant evaluates to. An array literal does not have the problem,
because its keys are evaluated: `[Slots::NAME => 'Ann']` is typed
`array<0, 'Ann'>`. A docblock keeps the spelling, and `shape_key_type` can
only call such a key an `array-key`.

**Fix:** Evaluate the constant. When the class is loadable and the constant
holds an integer or string literal, read the key as that value in
`shape_fits_array`, and in the structural list check
(`shape_keys_are_sequential`), which reads the same spelling as a string
key. A constant that cannot be evaluated stays an `array-key`, which does
not contradict a list.

## Laravel

No outstanding items.

## Blade

### B571. A `{{!!` with no `!!}` after it is read as a raw echo

**Impact: Low · Complexity: Low-Medium**

```blade
<p>{{!!$flag}}</p>
```

A double negation written without a space. Blade compiles a raw echo only
when a `!!}` follows the `{!!`, and with none the escaped echo compiles to
`e(!!$flag)`. `echo::open` (`src/blade/preprocessor/echo.rs`) reads every
`{{!!` as a literal `{` and a raw echo, so the template lowers to
`echo $flag}};` and reports a cascade of syntax errors. `{{ !!$flag }}` is
fine.

`open_escaped` (the `@{{!!` form), `mode_at`
(`src/blade/directive_completion.rs`), `blade_echo_delimiter_at`
(`src/blade/echo_delimiter.rs`) and `is_echo_start`
(`src/blade/signature.rs`) apply the same rule, so hover, directive
completion, the formatter and semantic tokens read the echo as a raw one
too.

**Fix:** Read a `{{!!` as a literal brace only when a `!!}` follows it.
`EchoCloses` already answers that for the preprocessor. The scanners ask per
`{`, so they need the position of the last `!!}` worked out once per scan,
not a search forward from every opener.

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
