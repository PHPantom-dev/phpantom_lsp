<?php

namespace ConstantArrayTypeIdentical;

use function PHPStan\Testing\assertType;

class Foo
{


	public function doFoo(string $s): void
	{

		assertType('bool', [$s] === [$s]);
		assertType('bool', [$s] !== [$s]);

		$a = [1];
		if (doFoo()) {
			$a[] = 2;
		}

		assertType('bool', [1] === $a);
		assertType('bool', $a === [1]);
		assertType('bool', [1, 2] === $a);

		assertType('bool', [1] !== $a);
		assertType('bool', [1, 2] !== $a);

		$b = [1];
		if (doFoo()) {
			$b[] = 2;
		}

		assertType('bool', $a === $b);
		assertType('bool', $a !== $b);
	}

	/**
	 * @param array{a?: int} $a
	 * @param array{b?: int} $b
	 */
	function doBar(array $a, array $b): void
	{
		assertType('bool', $a === $b);
		assertType('bool', $a == $b);

		assertType('bool', $a !== $b);
		assertType('bool', $a != $b);
	}

	/**
	 * @param array{c: int, d?: int} $a
	 * @param array{e: int, f?: int} $b
	 */
	function doBaz(array $a, array $b): void
	{

	}

}
