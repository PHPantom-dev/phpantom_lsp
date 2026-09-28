<?php

namespace PassedByReference;

use function PHPStan\Testing\assertType;

class Foo
{

	public function doFoo()
	{
		$arr = [1, 2, 3];
		reset($arr);

		preg_match('a', 'b', $matches);

		$s = '';
		$this->doBar($s);

		assertType('array{1, 2, 3}', $arr);
		assertType('array<string>', $matches);
		assertType('string', $s);
	}

	public function doBar(string &$s)
	{

	}

}
