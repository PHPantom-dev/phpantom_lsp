<?php

namespace ArrayKey;

use function PHPStan\Testing\assertType;

class Foo
{

	/**
	 * @param array-key $arrayKey
	 * @param array<array-key, string> $arrayWithArrayKey
	 */
	public function doFoo(
		$arrayKey,
		array $arrayWithArrayKey
	): void
	{
		assertType('(int|string)', $arrayKey);
		assertType('array<string>', $arrayWithArrayKey);
	}

}
