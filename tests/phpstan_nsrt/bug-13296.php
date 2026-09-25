<?php

namespace Bug13296;

use function PHPStan\Testing\assertType;

class Foo
{
	/**
	 * @param non-empty-array<Bar> $a
	 * @param non-empty-array<int> $b
	 */
	public function test($a, $b)
	{
		assertType('string', current($a)($b));
	}
}

class Bar
{
	/** @return string */
	public function __invoke($b)
	{
		return '';
	}
}
