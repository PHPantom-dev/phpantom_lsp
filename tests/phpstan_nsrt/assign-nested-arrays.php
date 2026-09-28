<?php

namespace AssignNestedArrays;

use function PHPStan\Testing\assertType;

class Foo
{

	public function doFoo(int $i)
	{
		$array = [];

		$array[$i]['bar'] = 1;
		assertType('non-empty-array<int, array{bar: 1}>', $array);

		$array[$i]['baz'] = 2;
		assertType('non-empty-array<int, array{bar: 1, baz: 2}>', $array);
	}

	public function doBar(int $i, int $j)
	{
		$array = [];

		$array[$i][$j]['bar'] = 1;
		$array[$i][$j]['baz'] = 2;

		echo $array[$i][$j]['bar'];
		echo $array[$i][$j]['baz'];

		assertType('non-empty-array<int, non-empty-array<int, array{bar: 1, baz: 2}>>', $array);
	}

}
