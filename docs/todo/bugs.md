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

PHPStan-style conditional return types (`@phpstan-return ($x is T ? A :
B)`) are how library stubs say what a call returns for the arguments it
gets. The WordPress stubs use them on some 150 functions, and their own
type tests are ported in `tests/wordpress_stubs/`: each entry below names how many of
those assertions it accounts for.

### B710. A conditional over a method template ignores its bound and does not narrow it

**Impact: Medium · Complexity: Medium**

```php
/**
 * @template T of int<0, max>
 * @param T|int<min, -1>|scalar|null $maybeint
 * @return ($maybeint is T ? T : …)
 */
function absint($maybeint) { … }
absint(5);   // want 5, get int<0, max>

/**
 * @template T of string
 * @return (T is non-falsy-string ? T|false : false)
 */
function sanitize_sql_orderby($orderby) { … }
sanitize_sql_orderby($string);   // want non-falsy-string|false, get string|false
```

Three gaps in how a template meets a condition: `$x is T` is never true,
even for an argument that is what `T` was bound to; a template bound to
an argument wider than its `of` bound (`mixed` passed for `T of
bool|string|int`) is not clamped to the bound; and the branch a condition
picks does not narrow the template it tested (`T` is
`non-falsy-string` inside the true branch above). 23 assertions.

### B719. A condition on `value-of<T>` or `key-of<T>` is judged against the argument itself

**Impact: Low-Medium · Complexity: Low-Medium**

```php
/**
 * @template T
 * @param T $value
 * @return (value-of<T> is string ? 'strings' : 'other')
 */
function f($value) { … }

f(['a' => 'x']);   // want 'strings', get 'other'
f('x');            // want 'other' (a string has no values), get 'strings'
```

A condition whose subject is a type operator over a template does not look
through the operator: the argument bound to `T` is judged as a whole. The
WordPress stubs' `wp_slash()` declares `array<key-of<T>, (value-of<T> is
string ? string : value-of<T>)>`, so slashing an array of strings is typed as
keeping each literal value rather than becoming `string`.

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
installer screen.

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

### B713. `@phpstan-assert` narrowing intersects refined string types wrongly

**Impact: Medium · Complexity: Low-Medium**

```php
/**
 * @template TUuid of string
 * @param TUuid $uuid
 * @phpstan-assert-if-true =TUuid&lowercase-string&non-falsy-string $uuid
 */
function wp_is_uuid($uuid, $version = null) { … }

$uuid = 'foo';
if (wp_is_uuid($uuid)) {
    $uuid;   // want 'foo', get never
}
```

Narrowing a `lowercase-string` by `=non-falsy-string`, or the literal
`'foo'` by `lowercase-string&non-falsy-string`, gives `never`, though
`'foo'` is both; and a template in the asserted type (`TUuid` above) is
left in the narrowed type instead of being replaced by what it was bound
to. 6 assertions.

### B714. An `@phpstan-impure` call is narrowed by an earlier check of the same call

**Impact: Low-Medium · Complexity: Low-Medium**

```php
/**
 * @return string|WP_Error
 * @phpstan-impure
 */
function get_password_reset_key($user) { … }

if (is_wp_error(get_password_reset_key($user))) {
    get_password_reset_key($user);   // want string|WP_Error, get WP_Error
}
```

A call to a function marked `@phpstan-impure` returns something new each
time, so a check on one call says nothing about the next. We narrow the
second call as if it were the first. 4 assertions.

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

### B720. An `(object)` cast written as an argument is typed `object`

**Impact: Low · Complexity: Low**

```php
stripslashes_from_strings_only((object) ['key' => 'value']);
// want object{key: 'value'}&stdClass, get object
```

The argument's type is read off its text (`arg_text_types.rs`), which
answers a cast by its keyword alone, so the shape `(object)` gives an
array is lost. Assigned to a variable first, the same expression goes
through the shared resolver and keeps it, though with its values
widened (`object{key: string}&stdClass`). Read the cast's operand
through the shared resolver, and keep the literal values the way an
array literal does. 1 assertion, from `stripslashes_from_strings_only()`.

### B721. An included file does not see the including scope's variables

**Impact: Low · Complexity: Medium**

```php
// main.php
function render(User $user): void {
    require __DIR__ . '/header.php';
    $title;            // want User, get nothing
}

// header.php
$title = $user;
```

The variables an included file leaves are worked out by walking its top
level on a scope of its own, once per file, so a value it reads from
the including scope is unknown there. Seed that walk with the variables
the including scope holds at the include, keyed on their types so a file
included from many places with the same scope is still walked once.
