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

### B573. Deferred code actions are sent to clients that cannot resolve them

**Impact: Low · Complexity: Low-Medium**

`handle_code_action` (`src/code_actions/mod.rs`) returns the deferred
actions (PHPStan quick fixes, extract function/method, extract variable,
extract constant, inline variable, convert to instance variable, remove
unused imports) with a `data` field and no `edit`, whatever the client
advertised. The spec only allows this when the client lists `edit` in
`textDocument.codeAction.resolveSupport.properties`; a client without it
applies an action as it was received. Nothing reads that capability
(nor `dataSupport`). Found with CodeLite before it supported
`codeAction/resolve`: the actions were listed, and picking one did
nothing.

**Fix:** Read `resolveSupport` and `dataSupport` at `initialize`, next
to `supports_file_rename`. When either is missing, compute the edit in
phase 1 through the same code `resolve_code_action` runs, and send it
without `data`.

### B574. A `[phpcs]` standard turns on PHPCS but not phpcbf

**Impact: Low · Complexity: Low**

`resolve_phpcs` (`src/phpcs.rs`) runs PHPCS whenever `[phpcs] standard`
is set, even with no ruleset file or `require-dev` entry, but
`Tool::detected` for phpcbf (`src/formatting/mod.rs`) only checks
`project_uses_phpcs`. A project configured that way with
`vendor/bin/phpcbf` installed gets PHPCS diagnostics and the built-in
formatter, though the comment there promises the linter and its fixer
never disagree. Treat a set standard as evidence in `detected` too,
still yielding to a `mago.toml` `[formatter]` table.

