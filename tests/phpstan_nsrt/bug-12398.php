<?php

namespace Bug12398;

use function PHPStan\Testing\assertType;

class Foo
{

	public int $test;

	public function doFoo(string $foo): void
	{
		$bar = 'foo';
		assertType('string', $$bar);
	}


	public function doBar(): void
	{
		$a = 'test';
		assertType('int', $this->$a);
	}

}

