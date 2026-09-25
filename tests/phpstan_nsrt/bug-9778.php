<?php

declare(strict_types=1);

namespace Bug9778;

const SOME_A = 0;

class HelloWorld
{
	public function sayHello(int $a): void
	{
		\PHPStan\Testing\assertType('int', $a);

		switch ($a) {
			case SOME_A:
				$b = "a";

				break;
			default:
				$b = "default";

				break;
		}

		\PHPStan\Testing\assertType('int', $a);
	}
}

