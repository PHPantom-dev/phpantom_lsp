<?php declare(strict_types = 1);

namespace Bug8917;

use function PHPStan\Testing\assertType;

class Foo
{
	public function test() {
		$array = array_column([[]], 'not_exist');

		assertType('array{}', $array);
		assertType('0', count($array));
		assertType('false', array_key_exists(0, $array));

		$array = array_column([[], ['a' => 1]], 'a');

		assertType('array{1}', $array);
		assertType('1', count($array));
		assertType('true', array_key_exists(0, $array));
	}
}
