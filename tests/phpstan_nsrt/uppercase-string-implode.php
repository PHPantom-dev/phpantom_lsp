<?php

namespace UppercaseStringImplode;

use function PHPStan\Testing\assertType;

class ImplodingStrings
{

	/**
	 * @param uppercase-string $ls
	 * @param array<string> $commonStrings
	 * @param array<uppercase-string> $uppercaseStrings
	 */
	public function doFoo(string $s, string $ls, array $commonStrings, array $uppercaseStrings): void
	{
		assertType('string', implode($s, $commonStrings));
		assertType('string', implode($s, $uppercaseStrings));
		assertType('string', implode($ls, $commonStrings));
	}
}
