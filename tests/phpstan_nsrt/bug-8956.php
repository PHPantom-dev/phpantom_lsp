<?php

namespace Bug8956;

use function PHPStan\Testing\assertNativeType;
use function PHPStan\Testing\assertType;

class Foo
{

	public function doFoo(): void
	{
		foreach (array_chunk(range(0, 10), 60) as $chunk) {
			assertType('array{0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10}', $chunk);
			assertNativeType('array', $chunk);
			foreach ($chunk as $val) {
				assertType('0|1|2|3|4|5|6|7|8|9|10', $val);
				assertNativeType('mixed', $val);
			}
		}
	}

	public function doBar(): void
	{
		assertType('array{array{0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10}}', array_chunk(range(0, 10), 60));
		assertNativeType('list<array>', array_chunk(range(0, 10), 60));
	}

}
