<?php

namespace LowercaseStringTrim;

use function PHPStan\Testing\assertType;

class Foo
{

	/**
	 * @param lowercase-string $lowercase
	 */
	public function doTrim(string $lowercase, string $string): void
	{
		assertType('string', trim($string));
		assertType('string', ltrim($string));
		assertType('string', rtrim($string));
		assertType('string', chop($string));
		assertType('string', trim($string, $lowercase));
		assertType('string', ltrim($string, $lowercase));
		assertType('string', rtrim($string, $lowercase));
		assertType('string', chop($string, $lowercase));
	}

}
