<?php

namespace InheritDocWithoutCurlyBracesFromInterface;

use function PHPStan\Testing\assertType;

class Foo extends FooParent implements FooInterface
{

	/**
	 * @inheritdoc
	 */
	public function doFoo($string)
	{
		assertType('string', $string);
	}

}

abstract class FooParent
{

}

interface FooInterface
{

	/**
	 * @param string $string
	 */
	public function doFoo($string);

}
