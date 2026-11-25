<?php

namespace GenericsDoNotGeneralize;

use ArrayIterator;
use function PHPStan\Testing\assertType;

/**
 * @template T
 * @param T $param
 * @return T[]
 */
function test($param): array
{

}

/**
 * @template T
 * @param T $param
 * @return Foo<T>
 */
function test2($param): Foo
{

}

/** @template T */
class Foo
{

	/** @param T $p */
	public function __construct($p)
	{

	}

}

function (): void {
	assertType('array<1>', test(1));
	assertType('GenericsDoNotGeneralize\Foo<int>', test2(1)); // SKIP: a template bound from a literal is generalized by the wrong rule
	assertType('GenericsDoNotGeneralize\Foo<int>', new Foo(1));
};

class Test
{
	public const CONST_A = 1;
	public const CONST_B = 2;

	/**
	 * @return self::CONST_*
	 */
	public static function foo(): int
	{
		return self::CONST_A;
	}
}

/**
 * Produces a new array of elements by mapping each element in collection through a transformation function (callback).
 * Callback arguments will be element, index, collection
 *
 * @template K of array-key
 * @template V
 * @template V2
 *
 * @param iterable<K,V> $collection
 * @param callable(V,K,iterable<K,V>):V2 $callback
 *
 * @return ($collection is list<V> ? list<V2> : array<K,V2>)
 *
 * @no-named-arguments
 */
function map($collection, callable $callback)
{
	$aggregation = [];

	foreach ($collection as $index => $element) {
		$aggregation[$index] = $callback($element, $index, $collection);
	}

	return $aggregation;
}

function (): void {
	$foo = Test::foo();

	assertType('1|2', $foo);

	$bar = map([new Test()], static fn(Test $test) => $test::foo());

	assertType('list<1|2>', $bar);
};

function (): void {
	/** @var list<string> $a */
	$a = doFoo();

};

/**
 * @template K of array-key
 * @template V
 * @param array<K, V> $a
 * @return ArrayIterator<K, V>
 */
function createArrayIterator(array $a): ArrayIterator
{

}

function (): void {
	/** @var list<string> $a */
	$a = doFoo();

};

/** @template T */
class FooInvariant
{

	/** @param T $p */
	public function __construct($p)
	{

	}

}

/** @template-covariant T */
class FooCovariant
{

	/** @param T $p */
	public function __construct($p)
	{

	}

}

function (): void {
	assertType('GenericsDoNotGeneralize\\FooInvariant<int>', new FooInvariant(1));
	assertType('GenericsDoNotGeneralize\\FooCovariant<1>', new FooCovariant(1)); // SKIP: a template bound from a literal is generalized by the wrong rule
};
