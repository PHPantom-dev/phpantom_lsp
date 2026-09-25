<?php

namespace ArrayObjectType;

use function PHPStan\Testing\assertType;

use AnotherNamespace\Foo;

class Test
{

	const ARRAY_CONSTANT = [0, 1, 2, 3];
	const MIXED_CONSTANT = [0, 'foo'];

	public function  doFoo()
	{
		/** @var Foo[] $foos */
		$foos = foos();

		foreach ($foos as $foo) {
			assertType('AnotherNamespace\Foo', $foo);
			assertType('AnotherNamespace\Foo', $foos[0]);
			assertType('0', self::ARRAY_CONSTANT[0]);
			assertType('\'foo\'', self::MIXED_CONSTANT[1]);
		}
	}

}
