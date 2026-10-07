<?php

namespace Bug5675;

use function PHPStan\Testing\assertType;

class Bar {}

/**
 * @template T
 */
class Hello
{
	/**
	 * @param (\Closure(Hello<T>): void)|array<array-key, int>|Bar $column
	 */
	public function foo($column): void
	{
		// ...
	}

	public function bar()
	{
		/** @var Hello<string> */
		$a = new Hello;

		$a->foo(function (Hello $h) : void {
			assertType('Bug5675\Hello<string>', $h);
		});
	}
}

/**
 * @template T
 */
class Hello2
{
	/**
	 * @param (\Closure(static<T>): void)|array<array-key, int>|Bar $column
	 */
	public function foo($column): void
	{
		// ...
	}

	public function bar()
	{
		/** @var Hello2<string> */
		$a = new Hello2;

		$a->foo(function (Hello2 $h) : void {
			\PHPStan\Testing\assertType('Bug5675\Hello2<string>', $h);
		});
	}
}
