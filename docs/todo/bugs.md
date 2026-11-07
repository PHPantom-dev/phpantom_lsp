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

### B735. `find_class_file_uri` finds a global class in any file with a class of its short name

**Impact: Low-Medium · Complexity: Low**

```php
// Tag.php
class Tag {}

// app.php
namespace App;
class Tag {}
// find_class_file_uri("Tag", "app.php") answers app.php
```

Asked for an unqualified (global) name, the lookup matches any class whose
short name is the same, namespace ignored, and checks the calling file
first. So a navigation that hands it a global FQN lands in the calling
file whenever that file declares a namespaced class of the same short name.
The namespaced case compares the namespace already; the global case should
require the class to be in no namespace the same way. Some callers may
pass a short name for a namespaced class and rely on the loose match, so
those need finding before the check tightens.

**Where to look:** `src/definition/member/file_lookup.rs`
(`find_class_file_uri`, the `None` arm of `class_in_file`).

## Code actions

No outstanding items.

## Miscellaneous

No outstanding items.
