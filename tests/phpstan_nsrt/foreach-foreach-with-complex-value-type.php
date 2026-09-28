<?php

namespace ForeachWithComplexValueType;

use function PHPStan\Testing\assertType;

class Foo
{

	/**
	 * @param (float|self)[] $list
	 */
	public function doFoo(array $list)
	{
		foreach ($list as $value) {
			assertType('float|ForeachWithComplexValueType\Foo', $value);
		}
	}

}
