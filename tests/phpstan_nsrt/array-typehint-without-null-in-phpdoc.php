<?php

namespace ArrayTypehintWithoutNullInPhpDoc;

use function PHPStan\Testing\assertType;

class Foo
{

	/**
	 * @return string[]
	 */
	public function doFoo(): ?array
	{
		return ['foo'];
	}

	public function doBar(): void
	{
		assertType('array<string>|null', $this->doFoo());
	}

	/**
	 * @param string[] $a
	 */
	public function doBaz(?array $a): void
	{
		assertType('array<string>|null', $a);
	}

}
