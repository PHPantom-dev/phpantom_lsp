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

## Conditional return types

No outstanding items.

## Standard-library return types

No outstanding items.

## Reachability

No outstanding items.

## Narrowing

### B692. `(int)` of a `ctype_digit()`-checked string is not non-negative

**Impact: Low · Complexity: Low-Medium**

```php
function wait(string $delay): void {
    if (!ctype_digit($delay)) {
        return;
    }
    sleep((int) $delay);   // expects int<0, max>, got int
}
```

A string that passes `ctype_digit()` holds only the digits 0-9, so casting
it to `int` cannot give a negative number, and `sleep()`'s
`int<0, max>` parameter accepts it. The narrowing leaves `$delay` a plain
`string` and the cast gives `int`. PHPStan reports nothing here only
because its function map declares `sleep()` and `usleep()` as taking a
plain `int`. phpstorm-stubs and Mago both declare `int<0, max>`, which
matches PHP 8 throwing a `ValueError` on a negative.

Found in a Laravel project's API client, which sleeps for the number of
seconds a rate-limit message names.

## Array types

### B726. A dynamic `class-string` key written into a shape widens every key to `string`

**Impact: Low · Complexity: Low-Medium**

```php
private const PAIRS = [NodeTraverser::class => [...]];

$pairs = self::PAIRS;
if (class_exists($className)) {
    $pairs[$className] = [...];
}
foreach (array_keys($pairs) as $class) {
    new ReflectionClass($class);
    // expects class-string<object>|object, got string
}
```

`array_keys(self::PAIRS)` alone gives `list<class-string<NodeTraverser>>`,
and `$className` is a `class-string` after the `class_exists()` check, so
every key is a class name. Once the dynamic write turns the shape into
`array<K, V>`, the shape's key reads as the literal `'NodeTraverser'`
rather than a class string, and joining it with `class-string` collapses
the key domain to `string`. A non-class literal key (`['x' => 1]` then
`$a[$cs] = 2`) also gives `array<string, …>`, where `'x'|class-string`
would be exact.

This started to show when `new ReflectionClass()`'s
`T|class-string<T>` parameter began rejecting a plain `string`.

**Where to change:** `merge_keyed_type_inner` and `join_element_types`
in `src/type_engine/variable/array_shape_writes.rs`. A shape key that
names a class should become `class-string<Foo>` there, the way
`merge_shape_key` already does through `class_name_key`, and the join
should keep a union of `class-string` members instead of widening it to
`string`.

Found in PHPStan's own build script that collects the classes its native
extension shadows (two sites).

### B728. A key written into a plain `array` turns it into a closed shape

**Impact: Medium · Complexity: Medium**

```php
/** @param array $args */
function add(string $id, array $args, mixed $default): void {
    $args['settings'] = $id;           // array{settings: string}
    $args['defaultValue'] = $default;  // array{settings: string, defaultValue: mixed}
    $wp_customize->add_control($id, $args);
    // expects array{…, settings?: array, …}, got array{settings: string, defaultValue?: mixed}
    // (undeclared key 'defaultValue')
}
```

`$args` holds whatever keys the caller passed, so after the writes it is
still an array with unknown keys that also has `settings` and
`defaultValue`. The write rebuilds it as a shape holding only the written
keys, which then gets checked as if those were all the keys there are:
an `undeclared key` report against a closed parameter shape, where a plain
`array` is accepted. A tracked `array<K, V>` base does not have this
problem: `merge_shape_key` folds the write into the key/value pair. A bare
`array` has no key type, so it falls through to the shape path meant for
`[]`.

**Where to change:** `merge_shape_key` in
`src/type_engine/variable/array_shape_writes.rs`. A bare `array` (or
`mixed[]`) base should keep its open key domain after the write. Ideally
it should also remember the written keys for later reads, the way
PHPStan's `hasOffsetValue` does. If that is not possible without an
intersection type, it should become `non-empty-array<array-key, …>`.

Found in a WordPress theme's customizer helper, which adds keys to the
caller's control arguments before passing them to
`WP_Customize_Manager::add_control()`.

## Laravel

No outstanding items.

## Blade

No outstanding items.

## Templates

No outstanding items.

## Framework names in strings

No outstanding items.

## Name resolution

### B704. A function declared in two files resolves to whichever is found first

**Impact: Medium · Complexity: Medium**

```php
// wp-admin/includes/noop.php, loaded only by load-scripts.php
function __() {}

// wp-includes/l10n.php, loaded everywhere else
function __( $text, $domain = 'default' ) { … }

__( 'Hello' );   // Expected 0 arguments, got 1
```

WordPress core declares nineteen of its functions a second time as
no-ops for its script loader. Calls across the project resolve to the
no-op, so with `extra-arguments` on WordPress core gets 17,435
argument-count reports (13 once `noop.php` is excluded, the same 13
PhpStorm reports), and hover, signature help, and return types all
describe the wrong declaration. PHP only ever loads one of them, and
nothing in the call says which.

When a name has several declarations, a call is valid if any of them
accepts it, and its return type is the join of theirs. Where one
declaration is plainly the real one (the other has an empty body and no
parameters, or sits in a file only one entry point includes), prefer it
for hover and go-to-definition, and offer the others alongside.

## Code actions

No outstanding items.

## Miscellaneous

No outstanding items.
