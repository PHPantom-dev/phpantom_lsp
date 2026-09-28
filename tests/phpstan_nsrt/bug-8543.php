<?php // lint >= 8.1

namespace Bug8543;

use function PHPStan\Testing\assertType;

class HelloWorld
{
	public readonly int $i;

	public int $j;

	public function invalidate(): void
	{
	}
}

function (HelloWorld $hw): void {
	$hw->i = 1;
	$hw->j = 2;
	assertType('1', $hw->i);
	assertType('2', $hw->j);

	$hw->invalidate();
	assertType('1', $hw->i);
	assertType('int', $hw->j);

	$hw = new HelloWorld();
	assertType('int', $hw->i);
	assertType('int', $hw->j);
};
