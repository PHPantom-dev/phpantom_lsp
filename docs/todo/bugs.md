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

### B743. A configurator confirmed while another is being confirmed keeps an incomplete answer

**Impact: Medium · Complexity: Medium**

Reproduced on `75f27a0ba` and on `d0b36afac` (before #568), so the
per-thread index reuse did not introduce it.

```php
// config/c00.php
function configure0(ContainerBuilder $container): void {
    $container->register('app.s0', ContainerBuilder::class);
}
// config/c01.php
function configure1(ContainerBuilder $container): void {
    $container->get('app.s0')->register('app.s1', ContainerBuilder::class);
}
// config/c02.php
function configure2(ContainerBuilder $container): void {
    $container->get('app.s1')->register('app.s2', ContainerBuilder::class);
}
// config/c03.php
function configure3(ContainerBuilder $container): void {
    $container->get('app.s2')->register('app.s3', ContainerBuilder::class);
}
```

The container index ends up with `app.s0` and `app.s1` only, so
`$container->get('app.s2')` and `'app.s3'` resolve to nothing everywhere.
Building the index confirms `c01`, which builds the index again, which
confirms `c02` while `c01` reads as declaring nothing. `app.s1` is then
missing, so `c02` declares nothing, and `typed_receiver_spans_for` caches
that answer even though `partial_answers()` moved while it was computed.
The outer build is correctly not kept, but the next one reads `c02`'s
cached answer and is. The answer only goes once the confirmed spans are
retired. When each file depends on one that sorts after it, as in
`chained_configurators` in `src/symfony/container.rs`, every nested answer
happens to be complete, which is why the existing test passes.

Not caching an answer computed while the partial count moved fixes it, but
every nested confirmation is partial in that sense, so the chain is then
recomputed at every level. The fix that keeps the cost down is a fixed
point: build the index from the untyped declarations, then confirm the
typed calls (`Contribution::typed`) against it, re-confirming until no
call is added.

## Name resolution

No outstanding items.

## Code actions

No outstanding items.

## Miscellaneous

No outstanding items.
