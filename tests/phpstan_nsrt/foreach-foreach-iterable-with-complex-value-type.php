<?php

namespace ForeachIterableWithComplexValueType;

use function PHPStan\Testing\assertType;

class Foo
{

	/**
	 * @param iterable<float|self> $list
	 */
	public function doFoo(iterable $list)
	{
		foreach ($list as $value) {
			assertType('float|ForeachIterableWithComplexValueType\Foo', $value);
		}
	}

}
