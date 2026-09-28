<?php

namespace Bug4982;

use function PHPStan\Testing\assertType;

class Foo
{

	public function doFoo(): void
	{
		if (!is_null($c = $this->doBar())) {
			assertType('int', $c);
		}
	}

	public function doBar(): ?int
	{

	}

}
