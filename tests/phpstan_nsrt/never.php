<?php // lint >= 8.1

namespace NeverTest;

use function PHPStan\Testing\assertType;

class Foo
{

	public function doFoo(): never
	{
		exit();
	}

	public function doBar()
	{
		assertType('never', $this->doFoo());
	}

	public function doBaz(?int $i)
	{
		if ($i === null) {
			$this->doFoo();
		}

		assertType('int', $i);
	}

}
