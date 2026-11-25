<?php

namespace Bug9131TypeInference;

use function PHPStan\Testing\assertType;

class Foo
{

	/**
	 * @param string[] $a
	 * @param array<string, string> $b
	 * @param array<int<0, max>, string> $c
	 * @param array<int<0, max>|string, string> $d
	 * @return void
	 */
	public function doFoo(
		array $a,
		array $b,
		array $c,
		array $d
	): void
	{
		$a[] = 'foo';
		assertType('non-empty-array<string>', $a);

		$b[] = 'foo';
		assertType('non-empty-array<int|string, string>', $b);

		$c[] = 'foo';

		$d[] = 'foo';
	}

}
