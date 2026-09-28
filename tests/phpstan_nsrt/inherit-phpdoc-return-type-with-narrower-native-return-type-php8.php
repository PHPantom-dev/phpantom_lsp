<?php // lint >= 8.0

namespace InheritPhpDocReturnTypeWithNarrowerNativeReturnTypePhp8;

use function PHPStan\Testing\assertType;

class Foo
{

	/**
	 * @return array<string>|positive-int|null
	 */
	public function doFoo(): array|int|null
	{

	}

}

class Bar extends Foo
{

	public function doFoo(): array|int
	{

	}

}

function (Bar $bar): void {
	assertType('array<string>|int<1, max>', $bar->doFoo());
};
