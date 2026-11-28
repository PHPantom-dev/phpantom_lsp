<?php declare(strict_types = 1);

namespace Bug4302b;

use function PHPStan\Testing\assertType;

class HelloWorld
{
	public function theMethod(MyInterface $interface, MyClass $class, MySubClass $subclass) {
		assertType('class-string|false', get_parent_class($interface));
		assertType('class-string<Bug4302b\\MyClass>|false', get_parent_class($class)); // PHPStan says `false`, but `$class` may be a subclass, whose parent is `MyClass` or below it
		assertType("class-string<Bug4302b\\MySubClass>|'Bug4302b\\\\MyClass'", get_parent_class($subclass)); // PHPStan says `'Bug4302b\MyClass'`, but `$subclass` may be a subclass, whose parent is `MySubClass` or below it
	}
}

interface MyInterface {}

class MyClass implements MyInterface {}

class MySubClass extends MyClass {}
