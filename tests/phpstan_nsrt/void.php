<?php

namespace VoidNamespace;

use function PHPStan\Testing\assertType;

class Foo
{

	public function doFoo(): void
	{
		assertType('null', $this->doFoo());
		assertType('null', $this->doBar());
		assertType('null', $this->doConflictingVoid());
	}

	/**
	 * @return void
	 */
	public function doBar(): void
	{

	}

	/**
	 * @return int
	 */
	public function doConflictingVoid(): void
	{

	}

}
