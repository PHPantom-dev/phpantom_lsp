<?php

namespace Bug10302TraitExtends;

use function PHPStan\Testing\assertType;

/**
 * @phpstan-require-extends SomeClass
 */
trait myTrait
{
	function test(): void
	{
		assertType('int', $this->x);
		assertType('string', $this->y);
		assertType('*ERROR*', $this->z);
	}
}

/**
 * @phpstan-require-extends SomeClass
 */
trait anotherTrait
{
}

class SomeClass {
	public int $x = 1;
	protected string $y = 'foo';
	private array $z = [];
}

function test(SomeClass $test): void
{
	assertType('int', $test->x);
	assertType('string', $test->y);
	assertType('array', $test->z);
}

class SomeSubClass extends SomeClass {}

class ValidClass extends SomeClass {
	use myTrait;
}

class ValidSubClass extends SomeSubClass {
	use myTrait;
}

class InvalidClass {
	use anotherTrait;
}
