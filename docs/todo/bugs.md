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

### B735. A class that does not load fails every parameter type it is passed to

**Impact: Medium · Complexity: Low-Medium**

```php
use ColinODell\PsrTestLogger\TestLogger; // package not installed

$logger = new TestLogger();
$factory->addLogger($logger); // expects Psr\Log\LoggerInterface, got TestLogger
```

When the argument's class cannot be loaded, nothing is known about its
parents or interfaces, so whether it satisfies `LoggerInterface` cannot be
decided. It is still reported as a mismatch, beside the `unknown_class`
already reported for the name itself, so one missing package gives two
errors per use. Drupal core's tests hit it wherever a dev-only package is
absent (about a dozen `addLogger()` calls in `projects/drupal`). An
argument whose class does not load should leave the subtype check
undecided rather than failed.

## Conditional return types

No outstanding items.

## Standard-library return types

No outstanding items.

## Reachability

No outstanding items.

## Narrowing

No outstanding items.

## Array types

### B736. `$list['key']` on an `ArrayObject` subclass ignores its `offsetGet()`

**Impact: Low-Medium · Complexity: Low-Medium**

```php
final class InstalledPackagesList extends \ArrayObject {
    public function offsetGet(mixed $key): ?InstalledPackage { /* ... */ }
}

$list['drupal/sub-module']->path; // Cannot check property $path, because type TValue could not be resolved
```

An array read on an object goes through its `offsetGet()`, so it has the
type the class's own override declares. The read takes `ArrayObject`'s
`TValue` instead, which the subclass never binds (it has no `@extends`),
and the unbound template name leaks into the diagnostic. Read the
element type from the `offsetGet()` the class resolves to, the way a
direct `$list->offsetGet('x')` call is, before falling back to the
template arguments. Seen in Drupal's package_manager tests.

## Laravel

No outstanding items.

## Blade

No outstanding items.

## Templates

No outstanding items.

## Framework names in strings

### B737. Test code does not see the entity types its test modules declare

**Impact: Medium · Complexity: Medium**

```php
// web/core/modules/config/tests/config_test/src/Entity/ConfigTest.php
// declares `config_test`
$storage = \Drupal::entityTypeManager()->getStorage('config_test');
$storage->load('foo')->status(); // Method EntityInterface::status() not found
```

Declarations under a `tests` folder are skipped (`is_test_fixture` in
`src/drupal/mod.rs`), because no real site enables a test module. The
tests that enable them do, though, so in test code an entity type a test
module declares leaves `getStorage()` with the declared
`EntityStorageInterface`, and what it loads is a bare `EntityInterface`.
That accounts for most of the ~150 `EntityInterface::get()` and
`::status()` reports in `projects/drupal`'s tests. Services already make
this exception (`is_test_services_file` in `src/drupal/services.rs`):
read test modules' entity types too, and let them type lookups made from
test code only, so production code keeps ignoring them.

## Name resolution

No outstanding items.

## Code actions

No outstanding items.

## Miscellaneous

### B738. A member chain rooted at `$this` inside a trait is never checked

**Impact: Low-Medium · Complexity: Medium**

```php
trait SwitchesWorkspace {
    protected function switchTo(int $id): void {
        $this->manager->missing();   // not reported
        $m = $this->manager;
        $m->missing();               // Method Manager::missing() not found
    }
}
class FirstTest { use SwitchesWorkspace; protected Manager $manager; }
```

Inside a trait, `unknown_member` skips every access whose subject starts
at `$this`, `self`, `static` or `parent`
(`subject_text_is_rooted_in_self` in
`src/diagnostics/unknown_members/access.rs`), whatever the subject
resolves to. That made sense when `$this` was only the trait, but it is
now also what the trait's users are all guaranteed to be, and a property
they declare is typed from their declarations, so the chain above
resolves to `Manager` and the same call is reported once it goes through
a variable. Only an access whose subject is nothing more than the trait
itself (a member the host is expected to provide) is still unknowable;
a chain whose subject resolves to a class should be checked like any
other.
