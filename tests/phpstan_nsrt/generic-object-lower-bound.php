<?php

namespace GenericObjectLowerBound;

use function PHPStan\Testing\assertType;

/** @template T of object */
interface Collection
{
	/** @param T $item */
	public function add(object $item): void;
}

/** @template-covariant T of object */
interface Collection2
{
	/** @param T $item */
	public function add(object $item): void;
}

class Cat
{
}

class Dog
{
}

class Foo
{

	/**
	 * @template T of object
	 * @param Collection<T> $c
	 * @param T $d
	 * @return T
	 */
	function doFoo(Collection $c, object $d)
	{
		$c->add($d);
	}

	/**
	 * @param Collection<Dog> $c
	 */
	function doBar(Collection $c): void
	{
		assertType(Cat::class . '|' . Dog::class, $this->doFoo($c, new Cat()));
	}

}

class Bar
{

	/**
	 * @template T of object
	 * @param Collection2<T> $c
	 * @param T $d
	 * @return T
	 */
	function doFoo(Collection2 $c, object $d)
	{
		$c->add($d);
	}

	/**
	 * @param Collection2<Dog> $c
	 */
	function doBar(Collection2 $c): void
	{
		assertType(Cat::class . '|' . Dog::class, $this->doFoo($c, new Cat()));
	}

}
