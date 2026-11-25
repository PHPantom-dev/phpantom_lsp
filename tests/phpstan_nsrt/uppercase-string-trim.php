<?php

namespace UppercaseStringTrim;

use function PHPStan\Testing\assertType;

class Foo
{

	/**
	 * @param uppercase-string $uppercase
	 */
	public function doTrim(string $uppercase, string $string): void
	{
		assertType('string', trim($string));
		assertType('string', ltrim($string));
		assertType('string', rtrim($string));
		assertType('string', chop($string));
		assertType('string', trim($string, $uppercase));
		assertType('string', ltrim($string, $uppercase));
		assertType('string', rtrim($string, $uppercase));
		assertType('string', chop($string, $uppercase));
	}

}
