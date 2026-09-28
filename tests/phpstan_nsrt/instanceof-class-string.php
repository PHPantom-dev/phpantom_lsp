<?php

namespace InstanceOfClassString;

use function PHPStan\Testing\assertType;

class Foo
{

	public function doFoo(Foo $foo): void
	{
		$class = get_class($foo);
		assertType('class-string<InstanceOfClassString\Foo>', $class);
		assertType(self::class, $foo);
		if ($foo instanceof $class) {
			assertType(self::class, $foo);
		} else {
			assertType(self::class, $foo);
		}
	}

}

class Bar extends Foo
{

	public function doBar(Foo $foo, Bar $bar): void
	{
		$class = get_class($bar);
		assertType('class-string<InstanceOfClassString\Bar>', $class);
		assertType(Foo::class, $foo);
		if ($foo instanceof $class) {
			assertType(self::class, $foo);
		} else {
			assertType('InstanceOfClassString\Foo', $foo);
		}
	}

}
