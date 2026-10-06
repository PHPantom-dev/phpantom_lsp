<?php

namespace Bug11928;

use function PHPStan\Testing\assertType;

function doFoo()
{
	$a = [2 => 1, 3 => 2, 4 => 1];

	$keys = array_keys($a, 1); // returns [2, 4]
	assertType('list<2|3|4>', $keys);

	$keys = array_keys($a); // returns [2, 3, 4]
	assertType('array{2, 3, 4}', $keys);
}

/**
 * @param array<1|2|3, 4|5|6> $unionKeyedArray
 * @param 4|5 $fourOrFive
 * @return void
 */
function doFooStrings($unionKeyedArray, $fourOrFive) {
	$a = [2 => 'hi', 3 => '123', 'xy' => 5];
	$keys = array_keys($a, 1);
	assertType("list<2|3|'xy'>", $keys);

	$keys = array_keys($a);
	assertType("array{2, 3, 'xy'}", $keys);

	$keys = array_keys($unionKeyedArray, 1);
	assertType("list<1|2|3>", $keys); // could be array{}

	$keys = array_keys($unionKeyedArray, 4);
	assertType("list<1|2|3>", $keys);

	$keys = array_keys($unionKeyedArray, $fourOrFive);
	assertType("list<1|2|3>", $keys);

	$keys = array_keys($unionKeyedArray);
	assertType("list<1|2|3>", $keys);
}

/**
 * @param array<int, int> $array
 * @param list<int> $list
 * @param array<string, string> $strings
 * @return void
 */
function doFooBar(array $array, array $list, array $strings) {
	$keys = array_keys($strings, "a", true);
	assertType('list<string>', $keys);

	$keys = array_keys($strings, "a", false);
	assertType('list<string>', $keys);

	$keys = array_keys($array, 1, true);
	assertType('list<int>', $keys);

	$keys = array_keys($array, 1, false);
	assertType('list<int>', $keys);

	$keys = array_keys($list, 1, true);
	assertType('list<int<0, max>>', $keys);

	$keys = array_keys($list, 1, true);
	assertType('list<int<0, max>>', $keys);
}
