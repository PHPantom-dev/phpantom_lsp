<?php

namespace Bug5322;

use function PHPStan\Testing\assertType;

class Foo
{

	function produceIntOrNull(): ?int
	{
		return rand(0, 2) === 0 ? null : 0;
	}

	function doFoo()
	{
		$int = null;
		while (!is_int($int)) {
			$int = $this->produceIntOrNull();
		}

		assertType('int', $int);
	}

	function doBar()
	{
		$int = $this->produceIntOrNull();
		while (!is_int($int)) {
			$int = $this->produceIntOrNull();
		}

		assertType('int', $int);
	}

}
