<?php

namespace Bug2735;

use function PHPStan\Testing\assertType;

class Dog {}

class Cat {}

/**
 * @template T
 */
class Collection {
	/** @var array<T> */
	protected $arr = [];

	/**
	 * @param array<T> $arr
	 */
	public function __construct(array $arr) {
		$this->arr = $arr;
	}

	/**
	 * @return T
	 */
	public function last()
	{
		if (!$this->arr) {
			throw new \Exception('bad');
		}
		return end($this->arr);
	}
}

/**
 * @template T
 * @extends Collection<T>
 */
class CollectionChild extends Collection {
}

$dogs = new CollectionChild([new Dog(), new Dog()]);
assertType('Bug2735\\CollectionChild<Bug2735\\Dog>', $dogs);

/**
 * @template X
 * @template Y
 */
class ParentWithConstructor
{

	/**
	 * @param X $x
	 * @param Y $y
	 */
	public function __construct($x, $y)
	{
	}

}

/**
 * @template T
 * @extends ParentWithConstructor<int, T>
 */
class ChildOne extends ParentWithConstructor
{

}

function (): void {
	$a = new ChildOne(1, new Dog());
	assertType('Bug2735\\ChildOne<Bug2735\\Dog>', $a);
};

/**
 * @template T
 * @extends ParentWithConstructor<T, int>
 */
class ChildTwo extends ParentWithConstructor
{

}

function (): void {
	$a = new ChildTwo(new Cat(), 2);
	assertType('Bug2735\\ChildTwo<Bug2735\\Cat>', $a);
};

/**
 * @template T
 * @extends ParentWithConstructor<T, T>
 */
class ChildThree extends ParentWithConstructor
{

}

function (): void {
	$a = new ChildThree(new Cat(), new Dog());
	assertType('Bug2735\\ChildThree<Bug2735\\Cat|Bug2735\\Dog>', $a);
};

/**
 * @template T
 * @template U
 * @extends ParentWithConstructor<T, U>
 */
class ChildFour extends ParentWithConstructor
{

}

function (): void {
	$a = new ChildFour(new Cat(), new Dog());
	assertType('Bug2735\\ChildFour<Bug2735\\Cat, Bug2735\\Dog>', $a);
};

/**
 * @template T
 * @template U
 * @extends ParentWithConstructor<U, T>
 */
class ChildFive extends ParentWithConstructor
{

}

function (): void {
	$a = new ChildFive(new Cat(), new Dog());
	assertType('Bug2735\\ChildFive<Bug2735\\Dog, Bug2735\\Cat>', $a);
};
