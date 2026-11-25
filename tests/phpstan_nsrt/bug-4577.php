<?php

namespace Bug4577;

use function PHPStan\Testing\assertType;

class Test
{

	public function test(\ReflectionClass $refClass): void
	{
		if ($refClass->isSubclassOf(Test::class)) {
			$instance = $refClass->newInstance();
			assertType(Test::class, $instance);
		}
	}

}
