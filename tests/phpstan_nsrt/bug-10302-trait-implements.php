<?php

namespace Bug10302TraitImplements;

use function PHPStan\Testing\assertType;

/**
 * @phpstan-require-implements SomeInterface
 */
trait myTrait
{
	protected function doIt() {
		$this->foo();
		$this->doesNotExist();
	}
}

interface SomeInterface
{
	public function foo(): string;
}

class SomeClass implements SomeInterface {
	use myTrait;

	public int $x;
	protected string $y;
	private array $z = [];

	public function foo(): string
	{
		return "hallo";
	}
}

function test(SomeClass $test): void
{
	assertType('int', $test->x);
	assertType('string', $test->y);
	assertType('array', $test->z);

	assertType('string', $test->foo());
}

class ValidImplements implements SomeInterface {
	public function foo(): string
	{
		return "hallo";
	}
}

class InvalidClass {
	use myTrait;
}
