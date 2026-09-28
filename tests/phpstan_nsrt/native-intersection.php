<?php // lint >= 8.1

namespace NativeIntersection;

use function PHPStan\Testing\assertType;

interface A
{

}

interface B
{

}

class Foo
{

	private A&B $prop;

	public function doFoo(A&B $ab): A&B
	{
		assertType('NativeIntersection\A&NativeIntersection\B', $this->prop);
		assertType('NativeIntersection\A&NativeIntersection\B', $ab);
		assertType('NativeIntersection\A&NativeIntersection\B', $this->doFoo($ab));
	}

}
