<?php

namespace DecimalIntString;

use function PHPStan\Testing\assertType;

class Foo
{

	/**
	 * @param decimal-int-string $s
	 */
	public function doFoo(string $s): void
	{
		$a = [$s => 1];

		assertType('bool', (bool) $s);

	}

	/**
	 * @param non-decimal-int-string $s
	 */
	public function doBar(string $s): void
	{
		$a = [$s => 1];

		assertType('bool', (bool) $s);

		assertType('float|int', $s + $s);
	}

	public function doBaz(string $s): void
	{
		$a = [$s => 1];

		$b = [];
		$b[$s] = 2;
		assertType('non-empty-array<string, 2>', $b);
	}

	/**
	 * @param non-decimal-int-string $s
	 */
	public function emptyStringIsNonDecimal(string $s): void
	{
		if ($s === '') {
			assertType("''", $s); // '' is a valid non-decimal-int-string
		}
	}

}
