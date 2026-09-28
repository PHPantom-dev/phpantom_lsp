<?php

namespace GetNativeType;

use function PHPStan\Testing\assertNativeType;
use function PHPStan\Testing\assertType;

class Foo
{

	public function doFoo(): void
	{
		assertType('string', $this->doBar());
		assertNativeType('string', $this->doBar());

		assertType('string', $this->doBaz());
		assertNativeType('mixed', $this->doBaz());

		assertType('non-empty-string', $this->doLorem());
		assertNativeType('string', $this->doLorem());
	}

	public function doBar(): string
	{

	}

	/**
	 * @return string
	 */
	public function doBaz()
	{

	}

	/**
	 * @return non-empty-string
	 */
	public function doLorem(): string
	{

	}

}
