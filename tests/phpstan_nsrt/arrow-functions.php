<?php

namespace ArrowFunctions;

use function PHPStan\Testing\assertType;

class Foo
{

	public function doFoo()
	{
		$x = fn(string $str): int => 1;
		$y = fn(): array => ['a' => 1, 'b' => 2];
		assertType('Closure(string): 1', $x);
		assertType('1', $x());
		assertType('array{a: 1, b: 2}', $y());
	}

}
