<?php

namespace CloneType;

use function PHPStan\Testing\assertType;

class Foo
{

	public function doFoo(?object $object, $mixed): void
	{
		assertType('object', clone $object);
		assertType('object', clone $mixed);
		assertType('static(CloneType\\Foo)', clone new $this);
		assertType('static(CloneType\\Foo)', clone new static);
		assertType(self::class, clone new self);
	}

}
