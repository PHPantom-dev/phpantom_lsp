<?php // lint >= 8.5

namespace ArrayFirstLast;

use function PHPStan\Testing\assertType;

/**
 * @param string[] $stringArray
 * @param non-empty-array<int, string> $nonEmptyArray
 * @param array{1: 'bar', baz: 'foo'} $arrayShape
 */
function doFoo(array $stringArray, array $nonEmptyArray, $mixed, $arrayShape): void
{
	assertType("'a'|'b'|'c'", array_first([1 => 'a', 0 => 'b', 2 => 'c']));
	assertType("'bar'|'foo'", array_first($arrayShape));
	assertType('string', array_first($nonEmptyArray));
	assertType('mixed', array_first($mixed));

	assertType("'a'|'b'|'c'", array_last([1 => 'a', 0 => 'b', 2 => 'c']));
	assertType("'bar'|'foo'", array_last($arrayShape));
	assertType('string', array_last($nonEmptyArray));
	assertType('mixed', array_last($mixed));
}
