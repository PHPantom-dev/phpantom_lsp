<?php // lint >= 8.0

declare(strict_types = 1);

namespace Bug12875a;

use function PHPStan\Testing\assertType;

interface HasFoo
{
	public function foo(): int;
}

interface HasBar
{
	public function bar(): int;
}

class HelloWorld
{
	/**
	 * @param "foo"|"bar" $method
	 * @param ($method is "foo" ? HasFoo : HasBar) $a
	 * @param ($method is "foo" ? HasFoo : HasBar) $b
	 */
	public function add(string $method, HasFoo|HasBar $a, HasFoo|HasBar $b): void
	{
		assertType('int', $a->{$method}());
		assertType('int', $b->{$method}());

		$addInArrow = fn () => assertType('int', $a->{$method}());

		$addInAnonymous = function () use ($a, $b, $method): void {
			assertType('int', $a->{$method}());
			assertType('int', $b->{$method}());
		};
	}
}
