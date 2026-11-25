<?php

namespace Bug10302InterfaceExtends;

use function PHPStan\Testing\assertType;

/**
 * @phpstan-require-extends SomeClass
 */
interface SampleInterface
{
}

class SomeClass {
	public int $x;
	protected string $y;
	private array $z = [];

	public function doFoo():int
	{
		return 1;
	}
}

function test(SampleInterface $test): void
{
	assertType('int', $test->x);
	assertType('string', $test->y);
	assertType('array', $test->z);

	assertType('int', $test->doFoo());
}

function testExtendedInterface(AnotherInterface $test): void
{
	assertType('int', $test->x);
	assertType('string', $test->y);
	assertType('array', $test->z);

	assertType('int', $test->doFoo());
}

interface AnotherInterface extends SampleInterface
{
}

class SomeSubClass extends SomeClass {}

class ValidClass extends SomeClass implements SampleInterface {}

class ValidSubClass extends SomeSubClass implements SampleInterface {}

class InvalidClass implements SampleInterface {}
