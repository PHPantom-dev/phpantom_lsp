<?php

namespace Bug2375;

use function PHPStan\Testing\assertType;

class Foo
{

	public function doFoo(
		$mixed,
		int $int,
		string $s,
		float $f
	): void
	{
		assertType('array{\'a\', \'b\', \'c\', \'d\'}', range('a', 'd'));
		assertType('array{\'a\', \'c\', \'e\', \'g\', \'i\'}', range('a', 'i', 2));

		// PHPantom is more precise: an empty or numeric string counts through numbers.
		assertType('non-empty-list<int|float|string>', range($s, $s));
	}

}
