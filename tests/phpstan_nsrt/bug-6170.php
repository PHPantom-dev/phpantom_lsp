<?php

namespace Bug6170;

use function PHPStan\Testing\assertType;

class HelloWorld
{
	/**
	 * @param array<non-empty-array> $array
	 **/
	public static function sayHello(array $array): bool
	{
		assertType('array<non-empty-array>', $array);
		if (rand(0,1)) {
			unset($array['bar']['baz']);
		}

		assertType('array<array>', $array);

		foreach ($array as $key => $value) {
			assertType('array', $value);
			assertType('int<0, max>', count($value));
		}

		return false;
	}
}
