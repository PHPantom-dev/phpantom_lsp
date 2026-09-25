<?php // lint >= 8.0

namespace Bug13312;

use function PHPStan\Testing\assertType;

function fooArr(array $arr): void {
	assertType('array', $arr);
	foreach ($arr as $v) {
	}
	assertType('array', $arr);

	for ($i = 0; $i < count($arr); ++$i) {
	}
	assertType('array', $arr);
}

/** @param list<mixed> $arr */
function foo(array $arr): void {
	assertType('list<mixed>', $arr);
	foreach ($arr as $v) {
	}
	assertType('list<mixed>', $arr);

	for ($i = 0; $i < count($arr); ++$i) {
	}
	assertType('list<mixed>', $arr);
}


function fooBar(mixed $mixed): void {
	assertType('mixed', $mixed);
	foreach ($mixed as $v) {
	}
	assertType('mixed', $mixed);

	foreach ($mixed as $v) {}

	assertType('mixed', $mixed);
}
