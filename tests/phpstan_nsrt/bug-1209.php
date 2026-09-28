<?php

namespace Bug1209;

use function PHPStan\Testing\assertType;

class HelloWorld
{
	/**
	 * @param mixed[]|string $value
	 */
	public function sayHello($value): void
	{
		$isArray = is_array($value);
		if($isArray){
			assertType('array<mixed>', $value);
		}
	}

	/**
	 * @param mixed[]|string $value
	 */
	public function sayHello2($value): void
	{
		$isArray = is_array($value);
		$value = 123;
		assertType('123', $value);
		if ($isArray) {
			assertType('123', $value);
		}

		assertType('123', $value);
	}
}
