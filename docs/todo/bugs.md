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

No outstanding items.
