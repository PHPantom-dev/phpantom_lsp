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

### B712. A type and its own subtype are not collapsed

**Impact: Low-Medium · Complexity: Medium**

```php
/** @var string $path */
if (wp_is_stream($path)) {   // @phpstan-assert-if-true =non-falsy-string $path
    // …
}
$path;   // want string, get string|non-falsy-string

/** @return string&'foo' */
function f() { … }
f();   // want 'foo', get string&'foo'
```

A union that holds a type and one of its subtypes is the supertype, and an
intersection of the two is the subtype. We keep both members, so the type
after an `if` without an `else` that narrowed a variable is spelled as
the two branches side by side, and `string&'foo'` stays as written instead
of `'foo'`. Hover reads worse, and a comparison against a declared type
has more members to check than it needs. 10 assertions of the WordPress
stubs fixtures (`tests/wordpress_stubs/`) are marked with this.

### B715. `===` between literals that cannot be equal is not `false`

**Impact: Low · Complexity: Low**

```php
/** @return 'HTTP/1.0'|'HTTP/1.1'|'HTTP/2' */
function wp_get_server_protocol() { … }

wp_get_server_protocol() === 'HTTP/1';   // want false, get bool
```

No member of the left side's union equals the right side's literal, so
the comparison is always `false`, and PHPStan types it so. We give
`bool`, which hides a typo in the compared string.

## Conditional return types

PHPStan-style conditional return types (`@phpstan-return ($x is T ? A :
B)`) are how library stubs say what a call returns for the arguments it
gets. The WordPress stubs use them on some 150 functions, and their own
type tests are ported in `tests/wordpress_stubs/`: each entry below names how many of
those assertions it accounts for.

### B705. The first of several `@phpstan-return` tags is read, not the last

**Impact: High · Complexity: Low**

```php
/**
 * @return WP_Post[]|int[]
 * @phpstan-return ( $args is array{ fields: 'ids', ... } ? int[] : WP_Post[] )
 * @phpstan-return ($args is array{fields: 'id=>parent'|'ids', ...} ? array<int, int> : array<int, \WP_Post>)
 */
function get_posts($args = null) { … }
```

PHPStan reads the last `@phpstan-return` of a docblock, so a stub that
adds a more precise tag below an older one gets the new one. We read the
first. `php-stubs/wordpress-stubs` has fifteen functions with two tags,
`get_posts()`, `wp_insert_post()`, `term_exists()`, `get_post_types()`,
`sanitize_post_field()`, `wp_upload_dir()` and `WP_Query::query()` among
them, so every one resolves through its outdated declaration. The same
holds for repeated `@phpstan-param` tags (`get_categories()`). 135
assertions.

### B706. A default written `\true`, `\false` or `\null` is not read by a conditional return type

**Impact: High · Complexity: Low**

```php
/** @phpstan-return ($display is true ? void : string) */
function the_title($before = '', $after = '', $display = \true) { … }

the_title();   // want void, get void|string
```

An omitted argument takes the parameter's default, and the condition is
decided by it. A default written `true` is read; written `\true`, the way
stub generators write it (and `\false`, `\null`), it is not, and the
condition is left undecided. A constant written `\OBJECT` is read. Every WordPress function
that echoes by default (`the_title()`, `comment_class()`,
`wp_loginout()`, …), and every `$wp_error = \false` flag
(`wp_insert_category()`, `wp_set_comment_status()`), gets both branches
where the default picks one. 31 assertions.

### B707. A conditional type inside a union, an intersection or a generic argument is not evaluated

**Impact: Medium-High · Complexity: Medium**

```php
/** @phpstan-return ($display is true ? void : string)|null */
function edit_term_link($link = '', $before = '', $after = '', $term = null, $display = true) { … }

/** @phpstan-return ($prefix is '' ? numeric-string : string)&non-falsy-string */
function wp_unique_id($prefix = '') { … }
```

A conditional is evaluated only when it is the whole return type. Inside
a union (`false|(…)`, `(…)|\WP_Error`), an intersection, or a generic
argument (`array<key-of<T>, (value-of<T> is string ? string : …)>`), it
stays in the type unevaluated, and hover shows its source text. This is
`get_term_by()`, `get_bookmark()`, `get_category()`, `get_tags()`,
`get_terms()`, `wp_get_post_terms()` and every other function that adds
`|\WP_Error` or `|false` to an argument-dependent result. 138
assertions.

### B699. An array-shape condition matches any array

**Impact: Medium-High · Complexity: Low-Medium**

```php
/** @phpstan-return ($args is array{fields: 'count', ...} ? int : string) */
function get_categories($args = '') { … }

get_categories(['fields' => 'names']);   // want string, get int
get_categories([]);                      // want string, get int
```

`$args is array{fields: 'count', ...}` is taken as true for every array,
whatever keys it has and whatever they hold, and for an omitted
argument whose default is `''`. A key the condition requires and the
argument lacks, or holds with a value outside the condition's, must make
the check false; an optional key or an argument whose keys are unknown
keeps it undecided.

Every WordPress function that takes an `$args` array decides its result
this way: `get_posts()` and `WP_Query::query()` without `fields` iterate
as `int` (22 of WordPress core's `scalar_member_access` reports),
`wp_list_pages(['echo' => false])` and the other echoing functions take
the first branch whatever `echo` says, and `wp_die()` with its default
arguments matches `array{exit: false}` and is read as returning, so code
after it stays reachable. 87 assertions, and one of PHPantom's own in
`output-constants.php`.

### B708. A condition over a refined type is misjudged

**Impact: Medium · Complexity: Medium**

```php
/** @phpstan-return ($value is int<min, 0> ? false : WP_User|false) */
function get_user($value) { … }
get_user(0);           // want false, get WP_User|false

/** @phpstan-return ($path is non-falsy-string ? bool : false) */
function path_is_absolute($path) { … }
path_is_absolute('foo');   // want bool, get false

/** @phpstan-return ($data is array<int, mixed> ? true : false) */
function wp_is_numeric_array($data) { … }
wp_is_numeric_array(['key' => 'value']);   // want false, get true
```

The check that decides a condition handles plain types and literals, and
gets these wrong:

- integer ranges (`int<min, 0>`, `int<min, -1>`) against an integer
  literal or another range;
- refined strings (`non-empty-string`, `non-falsy-string`,
  `lowercase-string`, `numeric-string`) against a literal that is one,
  which is judged not to be, also under `is not`;
- `numeric` and `empty`, which match nothing;
- `resource` and `object`, which are not ruled out of `string`;
- the key type of `array<K, V>`, which is ignored.

83 assertions, from `absint()`, `has_shortcode()`, `size_format()`,
`validate_file()`, `WP_Dependencies::query()` and others.

### B709. `void` from a conditional return type is not read as `null`

**Impact: Medium · Complexity: Low**

```php
/** @phpstan-return ($display is true ? void : string) */
function the_title($before = '', $after = '', $display = true) { … }

$title = the_title('', '', true);    // want null, get no type
$title = the_title('', '', $echo);   // want string|null, get string
```

A function declared `@return void` gives `null` to a variable assigned
from it. When `void` comes from a branch of a conditional instead, the
result has no type at all, a union with it drops it, and a union written
in the stub (`string|void`) keeps `void`. The value of a call is `null`
in all three. 54 assertions.

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
`non-falsy-string` inside the true branch above). 17 assertions.

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

### B711. A variadic template parameter binds only the first argument

**Impact: Low-Medium · Complexity: Low-Medium**

```php
/**
 * @template T
 * @param T ...$values
 * @return T
 */
function first_of(...$values) { … }

first_of(1, 'a');   // want 1|'a', get 1
```

A template that types a variadic parameter takes the union of every
argument passed to it. We bind it from the first one. 15 assertions,
through the `Faker::union()` helper the fixtures build union types with.

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

### B716. A call in another call's arguments keeps its receiver's first `@var` type

**Impact: Medium · Complexity: Medium**

```php
/** @template T */
class Box { /** @return T */ public function get() {} }

/** @var Box<int> $box */
$box = make();
strlen($box->get());

/** @var Box<string> $box */
$box = make();
$box->get();   // want string, get int
```

After a `@var` gives `$box` a new type, a call on it is resolved with the
old one when the same call appeared as an argument of another call
before the reassignment. It does not happen when the earlier call is a
statement of its own, or when the variable is retyped by an assignment
whose right-hand side has the new type. Found by the WordPress stubs
fixtures (`return/wp-rest-request.php`).

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
