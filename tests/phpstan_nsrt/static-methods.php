<?php

namespace StaticMethods;

use function PHPStan\Testing\assertType;

class Foo
{

	/** @return array<static> */
	public function method()
	{

	}

	/** @return array<static> */
	public function staticMethod()
	{

	}

	public function doFoo()
	{
		assertType('array<static(StaticMethods\Foo)>', $this->method());
		assertType('array<static(StaticMethods\Foo)>', $this->method()[0]->method());
		assertType('array<static(StaticMethods\Foo)>', self::staticMethod());
		assertType('array<static(StaticMethods\Foo)>', static::staticMethod());
	}

}

class Bar extends Foo
{

	public function doFoo()
	{
		assertType('array<static(StaticMethods\Bar)>', $this->method());
		assertType('array<static(StaticMethods\Bar)>', $this->method()[0]->method());
		assertType('array<static(StaticMethods\Bar)>', self::staticMethod());
		assertType('array<static(StaticMethods\Bar)>', static::staticMethod());
	}

}

function (Foo $foo, Bar $bar) {
	assertType('array<StaticMethods\Foo>', $foo->method());
	assertType('array<StaticMethods\Bar>', $bar->method());
	assertType('array<StaticMethods\Bar>', $bar->method()[0]->method());

	assertType('array<StaticMethods\Foo>', Foo::staticMethod());
	assertType('array<StaticMethods\Bar>', Bar::staticMethod());
};
