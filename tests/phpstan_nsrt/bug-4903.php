<?php

namespace Bug4903;

use function PHPStan\Testing\assertType;

class Foo
{

	/**
	 * @param array{a?:array{b:bool}} $options
	 */
	function f(array $options): void
	{
		assertType('bool', isset($options['a']['b']));
	}


	/**
	 * @param array{a?:array{b:bool}} $options
	 */
	function g(array $options): void
	{
		assertType('bool', isset($options['a'], $options['a']['b']));
	}

}
