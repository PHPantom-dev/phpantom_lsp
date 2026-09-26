<?php

namespace Bug5562;

use function PHPStan\Testing\assertType;

class Foo
{

	/**
	 * @template T of int|string
	 * @param T $test
	 * @return T
	 */
	public function foo($test)
	{
		assertType('T of int|string (method Bug5562\Foo::foo(), argument)', $test); // SKIP: a method template with a bound is shown as its bound inside the method
		$bar = $this->bar($test);
		assertType('T of int|string (method Bug5562\Foo::foo(), argument)', $bar); // SKIP: a method template with a bound is shown as its bound inside the method

		return $bar;
	}

	/**
	 * @template T of int|string
	 * @param T $test
	 * @return T
	 */
	public function bar($test)
	{
		return $test;
	}

}
