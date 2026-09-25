<?php

namespace LowercaseStringReplace;

use function PHPStan\Testing\assertType;

class ReplaceStrings
{

	/**
	 * @param string $s
	 * @param lowercase-string $ls
	 */
	public function doFoo(string $s, string $ls): void
	{
		assertType('string', str_replace($s, $s, $ls));
		assertType('string', str_replace($s, $ls, $s));
		assertType('string', str_replace($ls, $s, $ls));
		assertType('string', str_replace($ls, $ls, $s));

		assertType('string', str_ireplace($s, $s, $ls));
		assertType('string', str_ireplace($s, $ls, $s));
		assertType('string', str_ireplace($ls, $s, $ls));
		assertType('string', str_ireplace($ls, $ls, $s));

		assertType('string|null', preg_replace($s, $s, $ls));
		assertType('string|null', preg_replace($s, $ls, $s));
		assertType('string|null', preg_replace($ls, $s, $ls));
		assertType('string|null', preg_replace($ls, $ls, $s));

		assertType('string', substr_replace($s, $ls, 1));
		assertType('string', substr_replace($ls, $s, 1));
		assertType('string', substr_replace($s, $s, 1));
	}
}
