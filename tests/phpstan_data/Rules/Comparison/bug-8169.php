<?php // lint >= 8.0

declare(strict_types = 1);

namespace Bug8169;

use function PHPStan\Testing\assertType;

class HelloWorld
{
	/** @phpstan-assert string $var */
	public function assertString(mixed $var): void
	{
	}

	public function test(mixed $foo): void
	{
		assertType('mixed', $foo);
		$this->assertString($foo);
		assertType('string', $foo);
		$this->assertString($foo); // should report as always evaluating to true?
		assertType('string', $foo);
	}

	public function test2(string $foo): void
	{
		assertType('string', $foo);
		$this->assertString($foo); // should report as always evaluating to true?
		assertType('string', $foo);
	}

	public function test3(int $foo): void
	{
		assertType('int', $foo);
		$this->assertString($foo); // should report as always evaluating to false?
		assertType('*NEVER*', $foo); // SKIP: exhausted narrowing leaves the last type standing instead of never
	}
}
