<?php

namespace UnionMethods;

use function PHPStan\Testing\assertType;

class Foo
{

	public function doSomething(): self
	{

	}

}

class Bar
{

	public function doSomething(): self
	{

	}

}

class Baz
{

	/**
	 * @param Foo|Bar $something
	 */
	public function doFoo($something)
	{
		assertType('UnionMethods\Bar|UnionMethods\Foo', $something->doSomething());
		assertType('UnionMethods\Bar|UnionMethods\Foo', $something::doSomething());
	}

}

class FooStatic
{

	public static function doSomething(): self
	{

	}

}

class BarStatic
{

	public static function doSomething(): self
	{

	}

}
