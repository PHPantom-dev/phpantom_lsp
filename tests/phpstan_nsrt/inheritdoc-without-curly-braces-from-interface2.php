<?php

namespace InheritDocWithoutCurlyBracesFromInterface2;

use function PHPStan\Testing\assertType;

class Foo implements FooInterface
{

	/**
	 * @inheritdoc
	 */
	public function doBar($int)
	{
		assertType('int', $int);
	}

}



interface FooInterface extends BarInterface
{
}

interface BarInterface
{

	/**
	 * @param int $int
	 */
	public function doBar($int);

}
