<?php declare(strict_types = 1);

namespace Bug8056;

use function PHPStan\Testing\assertType;

$array = [];
$tmp = &$array;
$tmp[] = 'foo';

assertType("array{'foo'}", $array);
assertType("array{'foo'}", $tmp);

foreach ($array as $i) {
	assertType("'foo'", $i);
}
