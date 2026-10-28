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

### B698. A `bool` argument to a scalar parameter is reported outside `strict_types`

**Impact: High · Complexity: Low**

```php
// no declare(strict_types=1)
function label(string $s): void {}
function count_it(int $i): void {}

label(false);                  // expects string, got false
label(strpos($haystack, 'x')); // expects string, got int<0, max>|false
count_it(true);                // expects int, got true
```

Outside `strict_types`, PHP coerces a `bool` argument to a `string`,
`int`, or `float` parameter (`false` becomes `""` or `0`), so none of
these is an error at runtime. PHPStan accepts them too: its
`toCoercedArgumentType()` widens a scalar parameter to take `bool` in a
non-strict file. `coercions()` in `class_lookup/compatibility.rs`
already lets `int`/`float` reach `string` and numeric strings reach
numbers, but has no `bool` arm. WordPress core, which never declares
strict types, gets about 1,200 reports from this alone (`false` to
`string` is the largest single kind of argument mismatch it shows).

Two neighbours of the same arm are missing too: an integer range or
other refined `int` (`int<0, 255>` from `ord()`) is not coerced to
`string` the way a plain `int` is, and a scalar passed to a `bool`
parameter (`0`, `'yes'`) is reported although PHP coerces every scalar
to `bool`. Check the whole non-strict coercion table against PHP's
rules (and PHPStan's `toCoercedArgumentType()`) rather than adding arms
one at a time.

### B703. A bare `return;` is reported against a docblock `@return void|T`

**Impact: Low-Medium · Complexity: Low**

```php
/**
 * @return void|string Void if 'echo' argument is true.
 */
function wp_list_comments($args = []) {
    if ($echo) {
        return;   // Function with return type void|string must not return without a value
    }
    return $html;
}
```

A docblock that lists `void` in its return union says the function may
return nothing, so a bare `return;` is exactly what it describes. The
check treats the union as if `void` were not in it. WordPress core uses
`void|string` and `void|false` this way (18 reports).

### B699. A conditional return over an array shape matches arrays without the key

**Impact: Medium · Complexity: Low-Medium**

```php
/**
 * @phpstan-return ($args is array{ fields: 'ids', ... } ? int[] : WP_Post[])
 */
function get_posts($args = null) { … }

get_posts(['post_type' => 'post']);   // want WP_Post[], get int[]
```

The argument has no `fields` key, so it cannot be `array{fields: 'ids',
...}`, yet the condition is taken as true. Every `get_posts()` and
`WP_Query::query()` call without `fields` then iterates as `int`, and
each `$post->post_title` in the loop is reported as property access on
an `int` (22 of WordPress core's `scalar_member_access` reports).
`get_comments()` has the same shape (`$args is array{ count: true, ... }
? non-negative-int : …`), so its result is taken for an `int` too. A
required key the argument's shape lacks must make the check false; an
optional key (`fields?:`) or an argument whose keys are unknown keeps it
undecided.

## Standard-library return types

No outstanding items.

## Reachability

No outstanding items.

## Narrowing

### B700. `!empty($x->prop)` does not narrow the way `isset($x->prop)` does

**Impact: Medium · Complexity: Low-Medium**

```php
/** @return object|array|WP_Error */
function plugins_api() { … }

$result = plugins_api();
if (isset($result->slug)) {
    echo $result->slug;   // fine
}
if (! empty($result->name)) {
    echo $result->name;   // Property 'name' not found on class 'WP_Error'
}
```

`!empty($x->prop)` implies `isset($x->prop)`, so it must remove the same
union members: here `WP_Error`, which declares no `name` and has no
`__get`/`__isset`, and the `array`. The `isset` form already does this;
the `empty` form leaves the union whole. Found in WordPress core's plugin
installer screen. `(int)` of a `ctype_digit()`-checked string is not non-negative

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

No outstanding items.

## Laravel

### B695. A primitive cast on a nullable column drops the `null`

**Impact: Medium · Complexity: Low-Medium**

```php
// schema: `is_franchise` tinyint(1) DEFAULT NULL
class AdminUser extends Model
{
    protected $casts = ['is_franchise' => 'boolean'];
}

$user->is_franchise;   // want bool|null, get bool
```

`Model::castAttribute()` returns a `null` value untouched for every
primitive cast type (`bool`, `int`, `float`, `string`, `array`, dates, …),
so a nullable column keeps its `null` through the cast. We take the cast's
type and drop the column's nullability, so a null check on the property
reads as redundant and the `null` case is never offered. Laravel Idea gets
this right from the same schema.

Found on a Laravel project's admin user model, whose schema dump marks
the column nullable.

### B696. A relation whose related class comes from `config()` is typed `Model`

**Impact: Medium · Complexity: Medium**

```php
// spatie/laravel-permission's HasRoles
public function roles(): BelongsToMany
{
    return $this->morphToMany(config('permission.models.role'), 'model', …);
}

$user->roles;   // want Collection<int, Role>, get Collection<int, Model>
```

The related class is a `config()` value, and `config/permission.php` holds
a literal `Role::class` under that key. We already type config values, but
relationship body inference does not take a class from one, so every
relation declared this way (spatie/laravel-permission's `roles` and
`permissions` are the common case) falls back to the base `Model`. Laravel
Idea resolves both to the configured classes.

### B697. `analyze` and the editor disagree on a nullable `updated_at`

**Impact: Medium · Complexity: Medium**

On a model whose schema dump declares `updated_at timestamp NULL`, the
same binary reports `$model->updated_at` as `Illuminate\Support\Carbon`
from `analyze` and as `null|Illuminate\Support\Carbon` in the editor
(the schema is right, so the editor is). `created_at` (`NOT NULL`) is
`Carbon` in both. Some part of the model's column sources is loaded
differently, or in a different order, between the CLI and the language
server. Find which, and make both read the schema the same way.

Found on a Laravel project's admin user model by probing the same file
both ways.

## Blade

No outstanding items.

## Templates

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

### B701. Variables a `require`d file defines are reported as undefined

**Impact: Medium · Complexity: Medium**

```php
function core_upgrade(): string {
    require ABSPATH . WPINC . '/version.php';   // assigns $wp_version
    return $wp_version;                          // Undefined variable '$wp_version'
}
```

An `include`/`require` runs the file in the current scope, so every
variable its top level assigns exists afterwards. We report them as
undefined, even when the path is `__DIR__ . '/version.php'` and the file
is in the project. Where the path resolves, read the included file's
top-level assignments into the scope (with their types). Where it does
not, the scope after the include is open: stop reporting undefined
variables in it rather than guess. WordPress core gets 47 of these
(`$wp_version`, `$required_php_version`, the `$messages` an admin screen
`require`s).

### B702. `@uses` with a variable or a description is reported as an unknown function

**Impact: Low · Complexity: Low**

```php
/**
 * @uses global $wp_meta_boxes Used to retrieve registered meta boxes.
 * @uses $fn_get_css To run the function that gets the CSS.
 */
```

The first word after `@uses` is checked as a function name, giving
"Function 'global' not found" and "Function '$fn_get_css' not found".
`@uses` takes a structural element, and WordPress writes a global or a
variable there. Only judge a target that is shaped like one (a class, a
`Class::member`, or a `name()`); leave anything else alone.
