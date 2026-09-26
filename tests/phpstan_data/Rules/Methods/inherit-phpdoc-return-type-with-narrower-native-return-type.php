<?php

namespace InheritPhpDocReturnTypeWithNarrowerNativeReturnType;

use function PHPStan\Testing\assertType;

class Foo
{

	/**
	 * @return array<string>|null
	 */
	public function doFoo(): ?array
	{

	}

}

class Bar extends Foo
{

	public function doFoo(): array
	{

	}

}

function (Bar $bar): void {
	assertType('array<string>', $bar->doFoo());
};
