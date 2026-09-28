<?php

namespace ForeachDependentKeyValue;

use function PHPStan\Testing\assertType;

class Foo
{

	/**
	 * @param array{foo: int, bar: string} $a
	 */
	public function doFoo(array $a): void
	{
		foreach ($a as $key => $val) {
			assertType('int|string', $val);
			if ($key === 'foo') {
				assertType('int', $val);
			} else {
				assertType('string', $val);
			}

			if ($key === 'bar') {
				assertType('string', $val);
			} else {
				assertType('int', $val);
			}
		}
	}

}
