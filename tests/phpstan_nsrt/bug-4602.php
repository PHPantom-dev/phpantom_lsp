<?php

namespace Bug4602;

use function PHPStan\Testing\assertType;

class HelloWorld
{
	/** @param positive-int $limit */
	public function limit(int $limit): void
	{
	}

	public function doSomething(int $limit): void
	{
		if ($limit <= 1) {
			return;
		}
		assertType('int<1, max>', $limit - 1);
	}
}
