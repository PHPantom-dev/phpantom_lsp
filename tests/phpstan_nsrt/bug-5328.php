<?php

namespace Bug5328;

use function PHPStan\Testing\assertType;

class Foo
{

	function produceIntOrNull(): ?int
	{
		return rand(0, 2) === 0 ? null : 0;
	}

	function doBar()
	{
		$int = $this->produceIntOrNull();
		for ($i = 0; $i < 5 && !is_int($int) ; $i++) {
			$int = $this->produceIntOrNull();
		}

		assertType('int|null', $int);
	}

}
