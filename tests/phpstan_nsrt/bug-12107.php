<?php declare(strict_types = 1);

namespace Bug12107;

use LogicException;
use Throwable;

use function PHPStan\Testing\assertType;

class HelloWorld
{
	public function sayHello(Throwable $e1, LogicException $e2): void
	{
		if ($e1 instanceof $e2) {
			return;
		}

		assertType('Throwable', $e1);
		assertType('bool', $e1 instanceof $e2); // could be false
	}

	/** @param class-string<LogicException> $e2 */
	public function sayHello2(Throwable $e1, string $e2): void
	{
		if ($e1 instanceof $e2) {
			return;
		}


		assertType('Throwable', $e1);
		assertType('bool', $e1 instanceof $e2); // could be false
	}

	public function sayHello3(Throwable $e1): void
	{
		if ($e1 instanceof LogicException) {
			return;
		}

	}
}
