<?php

namespace GenericParent;

use function PHPStan\Testing\assertType;

interface Animal
{

}

interface Dog extends Animal
{

}

/**
 * @template T of Animal
 */
class Foo
{

	/** @return T */
	public function getAnimal(): Animal
	{

	}

}

/** @extends Foo<Dog> */
class Bar extends Foo
{

	public function doFoo()
	{
		assertType(Dog::class, parent::getAnimal());
		assertType(Dog::class, Foo::getAnimal());
	}

}

class E {}

/**
 * @template T of E
 */
class R {

	/** @return T */
	function ret() { return $this->e; } // nonsense, to silence missing return

	function test(): void {
		assertType('T of GenericParent\E (class GenericParent\R, argument)', self::ret());
		assertType('T of GenericParent\E (class GenericParent\R, argument)', $this->ret());
		assertType('T of GenericParent\E (class GenericParent\R, argument)', static::ret());
	}
}
