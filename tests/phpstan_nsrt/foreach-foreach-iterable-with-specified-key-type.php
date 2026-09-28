<?php

namespace ForeachWithGenericsPhpDocIterable;

use function PHPStan\Testing\assertType;

class Foo
{

	/**
	 * @param iterable<self|Bar, string|int|float> $list
	 */
	public function doFoo(iterable $list)
	{
		foreach ($list as $key => $value) {
			assertType('ForeachWithGenericsPhpDocIterable\Bar|ForeachWithGenericsPhpDocIterable\Foo', $key);
			assertType('float|int|string', $value);
		}
	}

}
