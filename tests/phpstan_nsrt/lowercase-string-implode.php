<?php

namespace LowercaseStringImplode;

use function PHPStan\Testing\assertType;

class ImplodingStrings
{

	/**
	 * @param lowercase-string $ls
	 * @param array<string> $commonStrings
	 * @param array<lowercase-string> $lowercaseStrings
	 */
	public function doFoo(string $s, string $ls, array $commonStrings, array $lowercaseStrings): void
	{
		assertType('string', implode($s, $commonStrings));
		assertType('string', implode($s, $lowercaseStrings));
		assertType('string', implode($ls, $commonStrings));
	}
}
