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

No outstanding items.

## Array types

No outstanding items.

## Laravel

No outstanding items.

## Blade

No outstanding items.

## Templates

No outstanding items.

## Framework names in strings

No outstanding items.

## Name resolution

### B761. A guarded copy of a vendor class hides the real one

**Impact: Medium · Complexity: Medium**

```php
// stubs/polyfill.php, listed under "autoload": {"files": [...]}
namespace Acme;
if (!class_exists(Widget::class)) {      // or `if (false)`
    class Widget { public function old(): void {} }
}

// vendor/acme/lib/src/Widget.php, found through PSR-4
namespace Acme;
class Widget { public function old(): void {} public function render(): string { return ''; } }

// src/UseIt.php
$w->render();   // "Method Acme\Widget::render() not found"
```

PHP never runs the guarded declaration here: `class_exists()` autoloads
the PSR-4 file first, and an `if (false)` body never runs at all. PHPantom
resolves `Acme\Widget` to the guarded copy instead, so every member only
the real class has is reported unknown, and hover, completion and
go-to-definition all read the stub. It happens wherever the guarded file
sits on disk and whichever guard it uses, as long as the real class is
reached through a vendor package's PSR-4 mapping. The same probe with the
real class under the project's own PSR-4 root resolved correctly.

The parser descends into `if` and block bodies on purpose
(`src/parser/classes/mod.rs`, so Doctrine's `ServiceEntityRepository`,
declared only under a guard, is indexed), but nothing records that the
declaration was conditional. A file from `autoload.files` is indexed
eagerly, so its copy is in the class index before the vendor PSR-4 file
is ever looked up, and the lookup stops there.

**Fix:** record on the declaration that it sits inside a conditional
block, and when such a declaration is the one a name resolves to, try
the Composer autoloader (classmap, then PSR-4) for an unconditional
declaration of the same name and prefer it, as PHP's autoloader does.
Keep the guarded declaration when the autoloader finds nothing, which is
the Doctrine case. Tusk PHP reported false positives from the same kind of
guarded declaration in Symfony's MongoDB and PHPUnit bridges and
Magento's Redis adapter (its CHANGELOG, "A class
declared inside a block"), which are the real-code places to verify the
fix against.

## Code actions

No outstanding items.

## Miscellaneous

No outstanding items.
