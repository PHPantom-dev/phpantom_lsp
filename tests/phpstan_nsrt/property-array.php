<?php

namespace PropertyArray;

use function PHPStan\Testing\assertType;

class Foo
{

	private $property;

	public function doFoo()
	{
		// PHPantom is more precise than PHPStan here: an untyped property is typed by what its class assigns it, which is only ever an array.
		assertType('array', $this->property);
		$this->property = [];
		assertType('array{}', $this->property);
		// PHPantom is more precise than PHPStan here: reading a key the empty array lacks gives null at runtime (with a warning).
		assertType('null', $this->property['foo']);
		$this->property['foo'] = 1;
		assertType('array{foo: 1}', $this->property);
		assertType('1', $this->property['foo']);
	}

}
