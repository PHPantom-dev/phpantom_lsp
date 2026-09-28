<?php

namespace Bug4586;

use function PHPStan\Testing\assertType;

class HelloWorld
{
	public function sayHello(): void
	{
		assertType('bool', isset($_SESSION));
	}
}
