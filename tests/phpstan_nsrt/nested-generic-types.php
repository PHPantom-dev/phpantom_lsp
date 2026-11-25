<?php

namespace NestedGenericTypes;

use function PHPStan\Testing\assertType;

/** @template T */
interface SomeInterface
{

}

/**
 * @template T of SomeInterface<T>
 * @template TT of SomeInterface<U>
 * @template U
 * @template V of SomeInterface<U>
 */
class Foo
{

	/** @var T */
	public $t;

	/** @var TT */
	public $tt;

	/** @var U */
	public $u;

	/** @var V */
	public $v;

	public function doFoo(): void
	{
		assertType('T of NestedGenericTypes\SomeInterface<NestedGenericTypes\T> (class NestedGenericTypes\Foo, argument)', $this->t);
		assertType('TT of NestedGenericTypes\SomeInterface<U (class NestedGenericTypes\Foo, argument)> (class NestedGenericTypes\Foo, argument)', $this->tt);
		assertType('U (class NestedGenericTypes\Foo, argument)', $this->u);
		assertType('V of NestedGenericTypes\SomeInterface<U (class NestedGenericTypes\Foo, argument)> (class NestedGenericTypes\Foo, argument)', $this->v);
	}

}

/**
 * @template T of SomeInterface<T>
 * @template TT of SomeInterface<U>
 * @template U
 * @template V of SomeInterface<U>
 * @param T $t
 * @param TT $tt
 * @param U $u
 * @param V $v
 */
function testFoo($t, $tt, $u, $v): void
{
	assertType('T of NestedGenericTypes\SomeInterface<NestedGenericTypes\T> (function NestedGenericTypes\testFoo(), argument)', $t);
	assertType('TT of NestedGenericTypes\SomeInterface<U (function NestedGenericTypes\testFoo(), argument)> (function NestedGenericTypes\testFoo(), argument)', $tt);
	assertType('U (function NestedGenericTypes\testFoo(), argument)', $u);
	assertType('V of NestedGenericTypes\SomeInterface<U (function NestedGenericTypes\testFoo(), argument)> (function NestedGenericTypes\testFoo(), argument)', $v);
}

/** @template T */
interface SomeFoo
{

}

/** @template T */
interface SomeBar
{

}

/**
 * @template T
 * @template U of SomeFoo<T>
 * @param U $foo
 * @return U
 */
function testSome($foo)
{

}

/**
 * @template T
 * @template U of SomeFoo<T>
 * @param U $foo
 * @return T
 */
function testSomeUnwrap($foo)
{

}

function (SomeFoo $foo): void
{
	assertType('NestedGenericTypes\SomeFoo', testSome($foo)); // PHPStan spells the same type `SomeFoo<mixed>`
	assertType('mixed', testSomeUnwrap($foo));
};

function (SomeBar $bar): void
{
	assertType('NestedGenericTypes\SomeFoo<mixed>', testSome($bar)); // SKIP: a template bound that names another template leaks that template's name
	assertType('mixed', testSomeUnwrap($bar));
};

/**
 * @param SomeFoo<string> $foo
 */
function testSome2($foo)
{
	assertType('NestedGenericTypes\SomeFoo<string>', testSome($foo));
	assertType('string', testSomeUnwrap($foo));
}

/**
 * @param SomeBar<string> $bar
 */
function testSome3($bar)
{
	assertType('NestedGenericTypes\SomeFoo<mixed>', testSome($bar)); // SKIP: a template bound that names another template leaks that template's name
	assertType('mixed', testSomeUnwrap($bar));
}

/**
 * @template T
 * @return T
 */
function unwrapWithoutParam()
{

}

function (): void {
	assertType('mixed', unwrapWithoutParam());
};

/**
 * @template T of SomeFoo<U>
 * @template U
 * @return T
 */
function unwrapGenericWithoutParam()
{

}

function (): void {
	assertType('NestedGenericTypes\SomeFoo<mixed>', unwrapGenericWithoutParam()); // SKIP: a template bound that names another template leaks that template's name
};

/**
 * @template T of SomeFoo
 * @param T $t
 * @return T
 */
function nonGenericBoundOfGenericClass($t)
{
	return $t;
}

function (SomeFoo $foo, SomeBar $bar): void {
	assertType(SomeFoo::class, nonGenericBoundOfGenericClass($foo));
	assertType(SomeFoo::class, nonGenericBoundOfGenericClass($bar));
};
