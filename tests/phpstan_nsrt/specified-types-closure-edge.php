<?php declare(strict_types = 1);

namespace SpecifiedTypesClouserEdge;
use function PHPStan\Testing\assertType;

function doFoo(callable $cb): void
{
}

class HelloWorld
{
	public function sayHello(): void
	{
		if (HelloWorld::someValue() === 5) {
			assertType('5', HelloWorld::someValue());
			doFoo(function () {
				assertType('int', HelloWorld::someValue());
			});
		}
	}

	public static function someValue(): int
	{

	}
}
