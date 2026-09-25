<?php

namespace ArrayShapeKeysStrings;

use function PHPStan\Testing\assertType;

class Foo
{

	/**
	 * @param array{
	 *  'namespace/key': string
	 * } $slash
	 * @param array<int, array{
	 *   "$ref": string
	 * }> $dollar
	 */
	public function doFoo(array $slash, array $dollar): void
	{
		assertType("array{'namespace/key': string}", $slash);
		assertType('array<int, array{\'$ref\': string}>', $dollar);
	}

}
