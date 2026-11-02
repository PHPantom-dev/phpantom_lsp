# PHPantom — PHPUnit & Pest

Known gaps in PHPantom's support for test code. What works today:
PHPUnit's assertions narrow types; data providers, `@depends`, and
coverage metadata navigate, rename, and report a name that matches
nothing; a data provider PHPUnit cannot call and a `setUp()` or
`tearDown()` that skips the parent's are reported; a class shows a lens
listing the tests that cover it or are named after it, and a test one
back to its class; Pest closures know the test case `Pest.php` binds
them to; Mockery mocks keep the mocked class; and the method a PHPUnit
or Mockery expectation names by string completes, navigates, renames,
and is reported when the mocked class has no such method.

PHPStan's PHPUnit extension (`phpstan/phpstan-phpunit`) is the
checklist of what a test author expects from diagnostics; PhpStorm's
PHPUnit support and Phpactor's PHPUnit and Prophecy integrations are
what they expect from an editor.

Items are ordered by **impact** (descending), then **complexity** (ascending)
within the same impact tier.

| Label      | Scale                                                                                                                  |
| ---------- | ---------------------------------------------------------------------------------------------------------------------- |
| **Impact** | **Critical**, **High**, **Medium-High**, **Medium**, **Low-Medium**, **Low**                                           |
| **Complexity** | **Low** (mechanical/boilerplate, no design decisions), **Medium** (self-contained, follows an existing pattern), **Medium-High** (spans modules, some new design), **High** (shared/core subsystem, correctness or performance tradeoffs), **Very High** (cross-cutting architecture, wide blast radius) |

---

## Out of scope for the server

**Running tests.** A test explorer, gutter run buttons, and re-running
one data set are what PHP Tools and PhpStorm offer, but LSP has no
request for them. They belong in the editor extensions (the VS Code
Testing API, Zed runnables), which is a separate decision from anything
in this document.

---

## TS3. Create a test for a class

**Impact: Medium · Complexity: Medium-High**

A code action on a class (or one of its methods) that writes the test
class at the path the class's test lens looks for (its path under its
`autoload` root, mirrored under an `autoload-dev` root with a `Test`
suffix): the right namespace, the project's
base test case, a `#[CoversClass]` attribute, and a test method per
public method chosen. PhpStorm and Phpactor both offer it. It shares the
file-creation path with
[A41](actions.md#a41-create-class-from-non-existing-name). For a Pest
project, write a Pest file instead.

---

## TS5. Assertion libraries

**Impact: Low-Medium · Complexity: Low**

webmozart/assert, beberlei/assert, and azjezz/psl narrow through the
`@psalm-assert` / `@phpstan-assert` annotations they ship, which the
engine reads. Nothing tests them by name, so the README cannot claim
them. Add an assertion fixture per library covering the common forms
(`Assert::string()`, `Assert::isInstanceOf()`, `Assert::notNull()`,
`Assert::allIsInstanceOf()` and the `nullOr*` variants,
`Psl\Type\shape()->coerce()`), and file each one that fails as a bug.

---

## TS7. Prophecy

**Impact: Low · Complexity: Low-Medium**

```php
$repo = $this->prophesize(OrderRepository::class);
$repo->findOpen()->willReturn([]);
$service = new Billing($repo->reveal());
```

Phpactor completes the mocked class's methods on an `ObjectProphecy`.
Check what PHPantom does with current Prophecy, which declares
`ObjectProphecy` generic: `reveal()` should give the mocked class, and
a call on the prophecy should complete the mocked class's methods and
give a `MethodProphecy`. PHPUnit 10 removed `prophesize()`, so Prophecy
is declining; fix only what is cheap.

---

## TS8. Port PHPStan's PHPUnit and Mockery extension tests

**Impact: Medium · Complexity: Low-Medium**

PHPStan's PHPUnit extension (`phpstan/phpstan-phpunit`) is this
document's checklist, and the Mockery support follows what
`phpstan/phpstan-mockery` types, but neither's tests are ported, as
the Symfony, Doctrine, and WordPress extensions' are. Their type tests
cover assertion narrowing, `createMock()` and `createStub()`
intersections, `getMockBuilder()` chains, and `Mockery::mock()` and
its expectation chains; the rule tests cover assertion misuse
(`assertSame(true, …)` where `assertTrue()` is meant, `assertEquals()`
where `assertSame()` is), mocked methods that do not exist, data
providers, coverage metadata, and `setUp()` that skips the parent. Port them into
`tests/phpstan_phpunit/` the same way: the `assertType()` fixtures run
by `tests/assert_type_runner.rs`, the rule tests as `.fixture` files,
and slices of `phpunit/phpunit` and `mockery/mockery` copied by
`scripts/extract-framework-stubs.php` as the stubs. File every
gap as a `// SKIP: reason` in this document or `docs/todo/`.

---

## TS9. PHPUnit and Pest demo project

**Impact: Medium · Complexity: Low-Medium**

The PHPUnit demos in `examples/php/definition.php` work without PHPUnit
installed, so the attributes, Pest, and Mockery are described there
rather than shown. Add `examples/testing/`, a Composer project in the
shape of `examples/symfony/` that installs `phpunit/phpunit`,
`pestphp/pest`, and `mockery/mockery`, with a small library under
`src/` and its tests under `tests/`: assertion narrowing, data
providers and `@depends` in both annotation and attribute form,
coverage attributes and the lens they feed, a `Pest.php` binding a test
case, and a Mockery mock. Put a "Try:" comment at each site and a line
in its README. Since the tests can actually run, `vendor/bin/phpunit`
and `vendor/bin/pest` passing is the runtime check the other demos
need an `assertions.php` for, and `analyze` on it must report no
errors.

---

## TS10. Benchmark for test code

**Impact: Low-Medium · Complexity: Low**

Laravel, Symfony, and Yii have Criterion benchmarks in `benches/`; test
code has none, though the coverage lens on a class looks through every
test that names it. Add `benches/testing.rs`: generate a few hundred
test classes covering a few dozen source classes, with data providers
and Pest files, and time the code lens on a covered class, completion of
a data provider name, and member completion inside a Pest closure.
Register it in `Cargo.toml` so `benches/ci_benchmarks.py` picks it up.
