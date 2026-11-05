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

### B729. Keys written into a plain `array` cannot be read back

**Impact: Low · Complexity: Medium-High**

```php
/** @param array $args */
function add(array $args, Foo $foo): void {
    $args['foo'] = $foo;
    $args['foo']->  // no completion: `$args` is still a plain `array`
}
```

A write into a plain `array` leaves it a plain `array`, since the caller
may have passed any other keys too. That keeps it from being checked as
if the written keys were all it held, but it also forgets what was
written, so reading the entry back gives `mixed`. PHPStan keeps both by
intersecting the array with `hasOffsetValue('foo', Foo)`.

An unsealed shape (`array{foo: Foo, ...}`) would remember the entry, but
the argument check then reports every written key a sealed parameter
shape does not declare, which a plain `array` is not reported for. The
fix needs a type that records the entries a write put there without
claiming they are checked against a sealed shape, or an argument check
that tells the two apart.

**Where to change:** `merge_shape_key` and `is_bare_array` in
`src/type_engine/variable/array_shape_writes.rs`, and
`undeclared_shape_keys` in `src/class_lookup/compatibility.rs`.

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

### B731. A method that returns `new static()` without a declared type gives its own class

**Impact: Low-Medium · Complexity: Low-Medium**

```php
class Base {
    public static function make() { return new static(); }
}
class Child extends Base {}

Child::make();   // Base, should be Child
```

With no native or docblock return type, the return type inferred from
the body binds `new static()` to the declaring class instead of keeping
it `static`, so a call through a subclass gives the parent. Declaring
`@return static` on the method or on an interface it implements gives
the right type.

### B732. `#[\AllowDynamicProperties]` does not allow dynamic properties

**Impact: Medium · Complexity: Low**

```php
#[\AllowDynamicProperties]
class Extension {}

function f(Extension $extension): int {
    return $extension->sort;   // Property 'sort' not found on class 'Extension'
}
```

A class marked `#[\AllowDynamicProperties]` (or one extending such a
class) takes any property, so reading or writing one it does not declare
is no unknown member. The attribute is only read by class-name
completion today. PHPStan's `ClassReflection::allowsDynamicProperties()`
walks the parent chain for the attribute. Drupal core's `Extension` is
the common case: `$module_data[$name]->sort` and `->requires` are
reported about thirty times on core once its services are typed.

### B733. A Drupal service a `ServiceProvider` alters keeps the class its YAML names

**Impact: Medium · Complexity: Medium**

```php
// core.services.yml: language_manager: { class: Drupal\Core\Language\LanguageManager }
// language module: LanguageServiceProvider::alter() calls
//   $container->getDefinition('language_manager')->setClass(ConfigurableLanguageManager::class)
\Drupal::service('language_manager')->getLanguageConfigOverride('fr', 'system.site');
// Method 'getLanguageConfigOverride' not found on class 'Drupal\Core\Language\LanguageManager'
```

A module's `<Module>ServiceProvider` class can replace the class of a
service another module declares, and the service is then built from the
replacement whenever that module is enabled. `\Drupal::service()` and
`$container->get()` are typed by the YAML class alone, so a call that
only the replacement declares is reported: about ninety times on core,
all for `language_manager`. Read the literal `setClass()` calls in
`ServiceProvider::alter()` methods and type the service as the union of
the declared class and every replacement.
