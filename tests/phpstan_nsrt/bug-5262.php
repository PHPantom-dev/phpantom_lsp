<?php declare(strict_types = 1);

namespace Bug5262;

use function PHPStan\Testing\assertType;

/**
 * @template T of TestBase
 * @param class-string<T> $testclass
 * @return T
 */
function test(bool $optional = false, string $testclass = TestBase::class): TestBase
{
	return new $testclass();
}

class TestBase
{
}

class TestChild extends TestBase
{
	public function hello(): string
	{
		return 'world';
	}
}

function runTest(): void
{
	assertType('Bug5262\TestChild', test(false, TestChild::class));
	assertType('Bug5262\TestChild', test(false, testclass: TestChild::class));
	assertType('Bug5262\TestChild', test(optional: false, testclass: TestChild::class));
	assertType('Bug5262\TestChild', test(testclass: TestChild::class, optional: false));
	assertType('Bug5262\TestChild', test(testclass: TestChild::class));
}
