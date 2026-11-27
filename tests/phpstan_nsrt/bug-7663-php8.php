<?php // lint >= 8.0

namespace Bug7663;

use function PHPStan\Testing\assertType;

class HelloWorld8
{
	/**
	 * @param 'de_DE'|'pretty-long' $str
	 */
	public function sayHello($str): void
	{
		assertType("''", substr('de_DE', 5, -5));
		assertType("'y'", substr('pretty-long', 5, -5));
		assertType("''|'y'", substr($str, 5, -5));
	}
}
