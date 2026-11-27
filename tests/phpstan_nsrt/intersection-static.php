<?php

namespace IntersectionStatic;

use function PHPStan\Testing\assertType;

interface Foo
{

	/**
	 * @return static
	 */
	public function returnStatic(): self;

}

interface Bar
{

}

interface Baz
{

	/**
	 * @return static
	 */
	public function returnStatic(): self;

}

class Lorem
{

	/**
	 * @param Foo&Bar $intersection
	 */
	public function doFoo($intersection)
	{
		assertType('IntersectionStatic\Bar&IntersectionStatic\Foo', $intersection);
		assertType('IntersectionStatic\Bar&IntersectionStatic\Foo', $intersection->returnStatic());
	}

	/**
	 * @param Foo&Baz $intersection
	 */
	public function doBar($intersection)
	{
		assertType('IntersectionStatic\Baz&IntersectionStatic\Foo', $intersection);
		assertType('IntersectionStatic\Baz&IntersectionStatic\Foo', $intersection->returnStatic());
	}

}

abstract class Ipsum implements Foo
{

	public function testThis(): void
	{
		assertType('static(IntersectionStatic\Ipsum)', $this->returnStatic());
		if ($this instanceof Bar) {
			assertType('static(IntersectionStatic\Ipsum)&IntersectionStatic\Bar', $this->returnStatic());
		}
		if ($this instanceof Baz) {
			assertType('static(IntersectionStatic\Ipsum)&IntersectionStatic\Baz', $this->returnStatic());
		}
	}

}
