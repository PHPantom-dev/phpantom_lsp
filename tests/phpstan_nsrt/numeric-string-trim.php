<?php

namespace NumericStringTrim;

use function PHPStan\Testing\assertType;

class Foo
{

	/**
	 * @param numeric-string $numericString
	 * @param string $string
	 */
	public function doTrim(string $numericString, string $string): void
	{
		assertType('string', trim($numericString, $string));
		assertType('string', ltrim($numericString, $string));
		assertType('string', rtrim($numericString, $string));


		assertType('string', trim($numericString, '0'));
		assertType('string', ltrim($numericString, '0'));
		assertType('string', rtrim($numericString, '0'));

		assertType('string', trim($numericString, '-0'));
		assertType('string', ltrim($numericString, '-0'));
		assertType('string', rtrim($numericString, '-0'));
	}

}
