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

### B582. A union of two or more non-null values counts as nullable

**Impact: Medium · Complexity: Low**

```php
class Node {}
class A extends Node {}
class B extends Node {}
function takesAorB(A|B $x): void {}
function f(Node $node): void {
    if ($node instanceof A) {
        $kind = 'a';
    } elseif ($node instanceof B) {
        $kind = 'b';
    } else {
        $kind = null;
    }
    if ($kind !== null) {
        takesAorB($node); // expects A|B, got Node
    }
}
```

The join after the chain records that `$kind` holding a value means
`$node` is `A|B`, but the `!== null` test never applies it. With one arm,
or with every arm assigning the same literal, it does.

`scope_value_is_nullable` (`cond_narrowing/null_identity.rs`) asks
`non_null_type().is_some()`, and `PhpType::non_null_type()` returns
`Some` for any union with at least two members besides `null`, whether
or not `null` is one of them. So `'a'|'b'` reads as nullable, and every
`NonNull` trigger and non-null implication on such a holder stays unmet.
All four callers in `cond_narrowing` go through it.

**Fix:** Ask whether the type has a `null` member instead. `accepts_null()`
is not a drop-in replacement, since it also counts `mixed`, which the
current check does not.

## Array types

No outstanding items.

## Laravel

No outstanding items.

## Blade

No outstanding items.

## Templates

No outstanding items.

## Miscellaneous

### B583. A fake formatter in the formatting tests can fail to start

**Impact: Low · Complexity: Low**

`formatting::tests::phpcbf_fixes_against_the_phpcs_standard` failed once
in a full `cargo test` run with "Failed to spawn phpcbf: Text file busy
(os error 26)" and passed on every rerun. `write_fake_tool` writes an
executable script that the test runs straight away. When another test
thread forks to start its own tool while the script is still open for
writing, the child holds the write handle until it execs, and starting
the script in that window fails with `ETXTBSY`.

**Fix:** Retry starting the tool on `ETXTBSY`, or write every fake tool
before any test can start a process.
