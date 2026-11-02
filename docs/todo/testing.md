# PHPantom — PHPUnit & Pest

Known gaps in PHPantom's support for test code. What works today:
PHPUnit's assertions narrow types; data providers, `@depends`, and
coverage metadata navigate, rename, and report a name that matches
nothing; covered classes show a lens listing their tests; Pest closures
know the test case `Pest.php` binds them to; and Mockery mocks keep the
mocked class.

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

## TS1. Mock method names

**Impact: High · Complexity: Medium**

```php
$repo = $this->createMock(OrderRepository::class);
$repo->method('findOpen')->willReturn([]);

$mailer = Mockery::mock(Mailer::class);
$mailer->shouldReceive('send')->once();

Cache::shouldReceive('get')->andReturn('value');
```

The method a mock is told to expect is a string naming a method of the
mocked class, and nothing reads it today: it does not complete,
navigate, rename with the method, or get reported when the class has no
such method. Every test suite that uses mocks has these.

- **PHPUnit**: `method()` on a value typed as the mocked class
  intersected with `MockObject` (or `Stub`), including through
  `expects(…)->method(…)`. The mocked class is the non-mock part of the
  intersection that `createMock()`, `createStub()`, and
  `getMockBuilder(…)->getMock()` return through PHPUnit's own
  `@template` declarations; confirm each resolves that way first.
- **Mockery**: `shouldReceive()`, `shouldNotReceive()`, `allows()`,
  `expects()`, and `shouldHaveReceived()` on a Mockery mock, and the
  `['method' => $return]` array form of `shouldReceive()` and
  `allows()`. Laravel's `Facade::shouldReceive()` names a method of the
  facade's target.
- **Features**: completion from the mocked class's methods, go-to-definition,
  find references and rename from the method, and an unknown-method
  diagnostic (PHPStan's `phpunit.mockMethod`). A class with `__call` takes
  any name, so the diagnostic must not fire for one.

**Where to look:** the string-key machinery Laravel and Symfony names
use (`src/virtual_members/laravel/string_keys.rs`), the Mockery patches
in `src/virtual_members/laravel/patches.rs`, and phpstan-phpunit's
`MockMethodCallRule`.

---

## TS2. Go to the test, and back

**Impact: Medium · Complexity: Medium**

PhpStorm jumps between a class and its test (Ctrl+Shift+T). PHPantom
links them only through coverage metadata: the lens on a covered class
lists the tests whose `#[CoversClass]` or `@covers` names it. Most
projects write no coverage metadata, so the link should fall back to
the naming convention: the class's path under its `autoload` PSR-4
root, mirrored under an `autoload-dev` root with a `Test` suffix
(`App\Billing\Invoice` → `Tests\Unit\Billing\InvoiceTest`), and Pest
test files named the same way.

LSP has no "go to test" request. Decide between a code lens (the
existing coverage lens can carry the convention match too) and a code
action, and what the test side shows.

**Where to look:** `src/references/covers.rs`, `src/code_lens.rs`.

---

## TS3. Create a test for a class

**Impact: Medium · Complexity: Medium-High** (depends on TS2)

A code action on a class (or one of its methods) that writes the test
class at the path TS2 would look for: the right namespace, the project's
base test case, a `#[CoversClass]` attribute, and a test method per
public method chosen. PhpStorm and Phpactor both offer it. It shares the
file-creation path with
[A41](actions.md#a41-create-class-from-non-existing-name). For a Pest
project, write a Pest file instead.

---

## TS4. Data provider declaration checks

**Impact: Low-Medium · Complexity: Low**

A data provider name that matches no method is reported. Two more
checks PHPStan's PHPUnit extension makes are not:

- The provider must be `public`, and from PHPUnit 10 on also `static`
  (`phpunit.dataProviderPublic`, `phpunit.dataProviderStatic`). Read the
  PHPUnit version from `composer.lock`, and offer a quick fix that adds
  the modifier.
- For an unknown name, offer a "Create data provider" quick fix that
  writes a `public static function name(): iterable` returning an empty
  array, as PhpStorm does.

**Where to look:** `src/symbol_map/extraction/phpunit.rs`.

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

## TS6. `setUp()` and `tearDown()` that skip the parent

**Impact: Low · Complexity: Low**

An overridden `setUp()`, `tearDown()`, `setUpBeforeClass()`, or
`tearDownAfterClass()` that never calls the parent's breaks test cases
whose base class does work there (Laravel's and Symfony's both do).
PHPStan reports it as `phpunit.callParent`. Report it, with a quick fix
that inserts the call.

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
gap as a `// SKIP: reason` in this document or `docs/todo/`; rule tests
TS1, TS4, or TS6 already cover point at it.

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
