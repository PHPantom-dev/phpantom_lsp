<?php

namespace UppercaseStringReplace;

use function PHPStan\Testing\assertType;

class ReplaceStrings
{

	/**
	 * @param string $s
	 * @param uppercase-string $us
	 */
	public function doFoo(string $s, string $us): void
	{
		assertType('string', str_replace($s, $s, $us));
		assertType('string', str_replace($s, $us, $s));
		assertType('string', str_replace($us, $s, $us));
		assertType('string', str_replace($us, $us, $s));

		assertType('string', str_ireplace($s, $s, $us));
		assertType('string', str_ireplace($s, $us, $s));
		assertType('string', str_ireplace($us, $s, $us));
		assertType('string', str_ireplace($us, $us, $s));

		assertType('string|null', preg_replace($s, $s, $us));
		assertType('string|null', preg_replace($s, $us, $s));
		assertType('string|null', preg_replace($us, $s, $us));
		assertType('string|null', preg_replace($us, $us, $s));

		assertType('string', substr_replace($s, $us, 1));
		assertType('string', substr_replace($us, $s, 1));
		assertType('string', substr_replace($s, $s, 1));
	}
}
