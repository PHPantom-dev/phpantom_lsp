<?php

namespace AssignmentInCondition;

use function PHPStan\Testing\assertType;

class Foo
{

	public function doFoo(): ?self
	{

	}

	public function doBar()
	{
		$foo = new self();
		if (null !== $bar = $foo->doFoo()) {
			assertType('AssignmentInCondition\Foo', $bar);
		}
	}

}
