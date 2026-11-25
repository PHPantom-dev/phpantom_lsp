<?php // lint >= 8.0

declare(strict_types = 1);

namespace Bug6251;

use function PHPStan\Testing\assertType;

class Foo
{
	function foo()
	{
		$var = 1;
		if (rand(0, 1)) {
			match(1) {
				1 =>  throw new \Exception(),
			};
		} else {
			$var = 2;
		}
		assertType('2', $var);
	}

	function bar($a): void
	{
		$var = 1;
		if (rand(0, 1)) {
			match($a) {
				'a' => throw new \Error(),
				default => throw new \Exception(),
			};
		} else {
			$var = 2;
		}
		assertType('2', $var);
	}

	function baz($a): void
	{
		$var = 1;
		if (rand(0, 1)) {
			match($a) {
				'a' => throw new \Error(),
				// throws UnhandledMatchError if not handled
			};
		} else {
			$var = 2;
		}
		assertType('2', $var);
	}

	function buz($a): void
	{
		$var = 1;
		if (rand(0, 1)) {
			match($a) {
				'a' => throw new \Exception(),
				default => var_dump($a),
			};
		} else {
			$var = 2;
		}
		assertType('1|2', $var);
	}
}
