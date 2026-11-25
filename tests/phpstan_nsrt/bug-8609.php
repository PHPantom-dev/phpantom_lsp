<?php

namespace Bug8609TypeInference;

use function PHPStan\Testing\assertType;

/**
 * @template T of bool
 */
class Foo
{

	/** @return (T is true ? 'foo' : 'bar') */
	public function doFoo(): string
	{

	}

}

class Bar
{

	/**
	 * @param Foo<true> $f
	 * @param Foo<false> $g
	 * @param Foo<bool> $h
	 * @param Foo $i
	 */
	public function doFoo(Foo $f, Foo $g, Foo $h, Foo $i): void
	{
		assertType('\'foo\'', $f->doFoo());
		assertType('\'bar\'', $g->doFoo());
		assertType('\'bar\'|\'foo\'', $h->doFoo());
		assertType('\'bar\'|\'foo\'', $i->doFoo());
	}

}
