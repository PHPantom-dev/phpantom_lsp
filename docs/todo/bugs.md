# PHPantom — Bug Fixes

Every bug below must be fixed at its root cause. "Detect the
symptom and suppress the diagnostic" is not an acceptable fix.
If the type resolution pipeline produces wrong data, fix the
pipeline so it produces correct data. Downstream consumers
(diagnostics, hover, completion, definition) should never need
to second-guess upstream output.

## B114. `Mockery::mock()` / `$this->mock()` drop the intersection with the mocked class

**Severity: Medium (false positives wherever a mock is assigned to a
property or return type typed as the concrete mocked class) · Confirmed
against real projects (a production Laravel codebase and a legacy PHP
app); PHPStan/Larastan report zero errors on the same lines**

```php
/** @var EpaymentService */
private $epaymentService;

protected function setUp(): void
{
    $mock = Mockery::mock(EpaymentService::class);
    $this->epaymentService = $mock; // "Property expects EpaymentService, got
                                     //  Mockery\MockInterface|Mockery\LegacyMockInterface"
}
```

```php
private function mockHelloRetailClient(): Client&MockInterface
{
    $mock = $this->mock(Client::class);
    // ...
    return $mock; // "Return type MockInterface is incompatible with
                   //  declared return type Client&MockInterface"
}
```

Mockery generates a dynamic subclass of the class passed to
`Mockery::mock()` (and Laravel's `TestCase::mock()` /
`partialMock()` / `spy()`, which delegate to it) when that class is
not `final`, so the returned object genuinely satisfies both the
concrete class and `MockInterface`. PHPantom currently resolves the
call's return type from `Mockery::mock()`'s own docblock
(`@return \Mockery\MockInterface`), dropping the class-string
argument entirely. PHPStan gets this right via
`phpstan-mockery`'s `MockDynamicReturnTypeExtension` (for
`Mockery::mock()`/`spy()`) and Larastan's `TestCaseExtension` (for
`$this->mock()`/`partialMock()`/`spy()`) — both are
argument-dependent return type extensions that intersect
`MockInterface` with the type of the class-string argument.

PHPantom has no equivalent mechanism for a handful of well-known
argument-dependent return types outside our own conditional
`@return` support. Where to add this: `completion/call_resolution.rs`
(or the return-type resolution path it feeds) should special-case
`Mockery::mock()`, `Mockery::spy()`, and, in Laravel projects, the
`TestCase` trait's `mock()`/`partialMock()`/`spy()` methods, and
intersect `MockInterface` with the resolved type of the first
argument when it is a `::class` reference (skip anything else, same
scope restriction as our other literal-argument-only resolvers).
