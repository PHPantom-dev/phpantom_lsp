# Fixture Test Format

Each `.fixture` file in this directory encodes a single test scenario. The
fixture runner (`tests/fixture_runner.rs`) parses the file, creates a test
backend, opens the PHP source, fires the appropriate LSP request, and checks
the declared expectations.

## File structure

A fixture has two sections separated by a `---` line:

```
// test: human-readable description
// feature: completion
// expect: bar(
---
<?php

class Foo {
    public function bar(): void {}
}

$f = new Foo();
$f-><>
```

### Header (above `---`)

| Directive | Required | Repeatable | Description |
|---|---|---|---|
| `// test:` | yes | no | Human-readable test name. Shown in test output. |
| `// feature:` | yes | no | One of `completion`, `hover`, `definition`, `signature_help`, `diagnostics`. |
| `// expect:` | depends | yes | **Completion/hover:** a label prefix (completion) or substring (hover) that must appear. |
| `// expect_absent:` | no | yes | **Completion:** a label prefix that must NOT appear. **Hover:** a substring that must NOT appear. |
| `// expect_hover:` | no | yes | `symbol => substring` fires a hover on `symbol` and checks the response contains `substring`. |
| `// expect_definition:` | depends | yes | `self:LINE` or `file:LINE` (1-based). |
| `// expect_sig_label:` | no | no | **Signature help:** the exact expected signature label. |
| `// expect_sig_active:` | no | no | **Signature help:** the expected active parameter index (0-based). |
| `// expect_sig_param:` | no | yes | **Signature help:** expected parameter labels in order. |
| `// ignore:` | no | no | Mark the test as ignored with a reason (e.g. `// ignore: needs todo.md §2`). |
| `// project:` | diagnostics | no | **Diagnostics:** the fixture project the files are analysed in, relative to `tests/`. |
| `// file:` | diagnostics | yes | **Diagnostics:** a file of the project to analyse, relative to the project. |
| `// codes:` | no | no | **Diagnostics:** comma-separated diagnostic codes the comparison covers. |
| `// expect_diagnostic:` | no | yes | **Diagnostics:** `[FILE:]LINE: message` that must be reported. |

Lines that don't match any directive are silently ignored, so you can add
plain comments:

```
// test: guard clause narrows type after early return
// feature: completion
// This is adapted from phpactor if-statement/type_after_return.test
// expect: barMethod(
```

### Body (below `---`)

PHP source with a single `<>` cursor marker indicating where the LSP request
fires. The runner strips `<>`, records its line/character offset, opens the
file, and sends the request.

```
<?php
$f = new Foo();
$f-><>
```

### Multi-file fixtures

For cross-file scenarios, the body can declare multiple files using
`=== path ===` delimiters. Exactly one file must contain the `<>` cursor.

```
// test: cross-file PSR-4 completion
// feature: completion
// expect: doWork(
---
=== src/Helper.php ===
<?php
namespace App;
class Helper {
    public function doWork(): void {}
}
=== src/Service.php ===
<?php
namespace App;
class Service {
    public function run(Helper $h): void {
        $h-><>
    }
}
```

## Feature-specific assertions

### `completion`

At least one `// expect:` or `// expect_absent:` is required.

- `// expect: bar(` passes if any completion label starts with `bar(`.
- `// expect_absent: secret` passes if no completion label starts with `secret`.

### `hover`

Two modes:

1. **Cursor hover:** use `// expect:` lines. The runner hovers at the `<>`
   cursor and checks the hover content contains each expected substring.
2. **Symbol hover:** use `// expect_hover: symbol => substring` lines. The
   runner finds `symbol` in the source, hovers over it, and checks the
   response.

Both modes also honour `// expect_absent:`, which asserts a substring is
missing from the hover text. Pair it with `// expect:` whenever the hover
renders a union, since a bare `// expect: Foo` passes even when the hover
also shows `Bar`.

### `definition`

Use `// expect_definition:` lines.

- `self:12` means the definition is on line 12 (1-based) of the cursor file.
- `src/Foo.php:5` means the definition is in `src/Foo.php` on line 5.

### `signature_help`

Use any combination of:
- `// expect_sig_label: (string $name, int $age): void`
- `// expect_sig_active: 0`
- `// expect_sig_param: string $name`
- `// expect_sig_param: int $age`

At least one signature help assertion is required.

### `diagnostics`

A diagnostics fixture has no body and no cursor. It analyses files of a
fixture project (`tests/phpstan_symfony/`, `tests/phpstan_doctrine/`), the
way the PHPStan extensions' rule tests analyse their data files, with the
project's classes and configuration indexed first:

```
// test: A controller reading the query bag calls only methods that exist
// feature: diagnostics
// project: phpstan_symfony
// file: rules/data/input_bag.php
// codes: unknown_member
---
```

Only the diagnostics whose code `// codes:` names are compared, against the
`// expect_diagnostic:` lines: each one must be reported on its line (1-based)
with exactly its message, and nothing else with those codes may be. `FILE`
defaults to the first `// file:`, and `\n` in a message stands for a line
break.

A trailing `// SKIP: reason` marks an expectation PHPantom does not meet yet:
it is not required, and a diagnostic on its line is not held against the
fixture.

```
// expect_diagnostic: 42: Command "example-rule" does not define argument "undefined". // SKIP: console arguments and options are not read from configure()
```

The ported rule tests keep the upstream message and mark the gap
this way rather than loosening the comparison. A fixture whose rule has no
PHPantom counterpart yet names no codes and compares nothing; it records
what to check once one exists.

## Directory layout

Fixtures are organised by category, mirroring the Phpactor inference test
directories where applicable:

```
tests/fixtures/
  generics/
  narrowing/
  foreach/
  virtual_member/
  type/
  ...
```

## Ignored fixtures

Use `// ignore:` to mark a test as ignored. The runner prints the reason and
moves on. This is useful for tests that cover planned features.

```
// test: constructor argument infers template type
// feature: completion
// ignore: needs todo.md §2 (function-level @template)
// expect: bar(
---
...
```

## Porting from Phpactor

Phpactor `.test` files use `wrAssertType('Type', $expr)` to assert on inferred
types. Since PHPantom doesn't expose a "resolve type at offset" API, translate
these into the closest LSP feature:

| Phpactor assertion | PHPantom fixture approach |
|---|---|
| `wrAssertType('Foo', $x->bar())` | `// feature: completion` on `$x->bar()-><>` and check for `Foo` members |
| `wrAssertType('Foo', $x)` | `// feature: hover` with `// expect_hover: $x => Foo` |
| Type after narrowing | Completion after the narrowing point, checking that the narrowed type's members appear |

When a Phpactor fixture has multiple `wrAssertType` calls at different
offsets, split it into separate `.fixture` files (one per cursor position).
Name them clearly so the connection is obvious.