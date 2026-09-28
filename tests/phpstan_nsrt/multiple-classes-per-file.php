<?php

namespace MultipleClasses;

use function PHPStan\Testing\assertType;

class Foo
{

	/**
	 * @param self $self
	 */
	public function doFoo($self)
	{
		assertType('MultipleClasses\Foo', $self);
	}

	/**
	 * @return self
	 */
	public function returnSelf()
	{

	}

}

class Bar
{

	/**
	 * @param self $self
	 */
	public function doFoo($self)
	{
		assertType('MultipleClasses\Bar', $self);
	}

	/**
	 * @return self
	 */
	public function returnSelf()
	{

	}

}
