<?php

namespace StaticProperties;

use function PHPStan\Testing\assertType;

class Foo
{

	/** @var array<static> */
	public $prop;

	/** @var array<static> */
	public static $staticProp;

	public function doFoo()
	{
		assertType('array<static(StaticProperties\Foo)>', $this->prop);
		assertType('array<static(StaticProperties\Foo)>', $this->prop[0]->prop);
		assertType('array<static(StaticProperties\Foo)>', self::$staticProp);
		assertType('array<static(StaticProperties\Foo)>', static::$staticProp);
	}

}

class Bar extends Foo
{

	public function doFoo()
	{
		assertType('array<static(StaticProperties\Bar)>', $this->prop);
		assertType('array<static(StaticProperties\Bar)>', $this->prop[0]->prop);
		assertType('array<static(StaticProperties\Bar)>', self::$staticProp);
		assertType('array<static(StaticProperties\Bar)>', static::$staticProp);
	}

}

function (Foo $foo, Bar $bar) {
	assertType('array<StaticProperties\Foo>', $foo->prop);
	assertType('array<StaticProperties\Bar>', $bar->prop);

	assertType('array<StaticProperties\Foo>', Foo::$staticProp);
	assertType('array<StaticProperties\Bar>', Bar::$staticProp);
};
