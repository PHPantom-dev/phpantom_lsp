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

### B760. A nullable or union receiver never confirms a typed key

**Impact: Low-Medium · Complexity: Low**

```php
public function __construct(
    private ?Factory $views,
    private Factory|Mailer $renderer,
) {}

public function show(): void
{
    $this->views->make('pages.home');    // no view key
    $this->renderer->make('pages.home'); // no view key
}
```

`typed_receiver_key_kind` (`src/blade/typed_receiver.rs`) asks whether
the receiver's whole type is a subtype of the target class.
`is_subtype_of_typed` requires every member of a union to pass, and
treats `?Factory` as `Factory|null`, so `null` fails it, and so does
the other half of a `Factory|Mailer`. The key is then not confirmed:
there is no go-to-definition, completion or reference for it, and no
missing-template diagnostic. A call is only reached when its receiver
is not `null`, so drop `null` from the receiver's type, then check each
remaining member separately and take the first that names a target. Do
this once in `typed_receiver_key_kind`, for every receiver class (view
factories, mailables, Symfony containers, Drupal and Yii targets), not
per receiver.

## Name resolution

No outstanding items.

## Code actions

No outstanding items.

## Miscellaneous

No outstanding items.
