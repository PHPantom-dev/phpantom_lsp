<?php

namespace Bug5846;

use function PHPStan\Testing\assertType;

class Foo
{
	public function test(): void
	{
		$arr = [
			'a' => 1,
			'b' => 'bee',
		];
		$data = array_merge($arr, $arr);
		$data2 = array_merge($arr);

		assertType('array{a: 1, b: \'bee\'}', $arr);
		assertType('array{a: 1, b: \'bee\'}', $data);
		assertType('array{a: 1, b: \'bee\'}', $data2);
	}
}
