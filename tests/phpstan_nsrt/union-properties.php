<?php

namespace UnionProperties;

use function PHPStan\Testing\assertType;

class Foo
{

	/** @var self */
	private $doSomething;

}

class Bar
{

	/** @var self */
	private $doSomething;

}

class Baz
{

	/**
	 * @param Foo|Bar $something
	 */
	public function doFoo($something)
	{
		assertType('UnionProperties\Bar|UnionProperties\Foo', $something->doSomething);
	}

}

class FooStatic
{

	/** @var self */
	private static $doSomething;

}

class BarStatic
{

	/** @var self */
	private static $doSomething;

}
