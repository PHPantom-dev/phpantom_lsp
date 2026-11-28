<?php

namespace ParentClass;

use function PHPStan\Testing\assertType;

class Foo
{

	public function doFoo()
	{
		assertType('false', get_parent_class());
		assertType('class-string<ParentClass\Foo>|false', get_parent_class($this)); // PHPStan says `false`, but `$this` may be a subclass, whose parent is `Foo` or below it
		assertType('\'ParentClass\\\\Foo\'', get_class());
	}

}

class Bar extends Foo
{

	use FooTrait;

	public function doBar()
	{
		assertType('\'ParentClass\\\\Foo\'', get_parent_class());
		assertType('class-string<ParentClass\\Bar>|\'ParentClass\\\\Foo\'', get_parent_class($this)); // PHPStan says `'ParentClass\Foo'`, but `$this` may be a subclass, whose parent is `Bar` or below it
	}

}

function (string $s) {
	assertType('false', get_parent_class());
	assertType('class-string|false', get_parent_class($s));
	assertType('false', get_parent_class(\ParentClass\Foo::class));
	assertType('class-string|false', get_parent_class(NonexistentClass::class));
	assertType('class-string|false', get_parent_class(1));
	assertType('\'ParentClass\\\\Foo\'', get_parent_class(\ParentClass\Bar::class));
	assertType('false', get_class());
};

trait FooTrait
{

	public function doBaz()
	{
		assertType('class-string|false', get_parent_class());
		assertType('class-string|false', get_parent_class($this));
	}

}
