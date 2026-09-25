<?php

namespace ArrayFilterStringCallables;

use function PHPStan\Testing\assertType;

/**
 * @param array<int, int> $list1
 * @param array<int, int> $list2
 * @param array<int, int> $list3
 */
function alwaysEvaluatesToFalse(array $list1, array $list2, array $list3): void
{
	$filtered1 = array_filter($list1, 'is_string');

	$filtered2 = array_filter($list2, 'is_string', ARRAY_FILTER_USE_KEY);

	$filtered3 = array_filter($list3, 'is_string', ARRAY_FILTER_USE_BOTH);
}

/**
 * @param array<int|string, int|string> $map1
 * @param array<int|string, int|string> $map2
 * @param array<int|string, int|string> $map3
 */
function filtersString(array $map1, array $map2, array $map3): void
{
	$filtered1 = array_filter($map1, 'is_string');
	assertType('array<int|string, string>', $filtered1);

	$filtered2 = array_filter($map2, 'is_string', ARRAY_FILTER_USE_KEY);
	assertType('array<string, int|string>', $filtered2);

	$filtered3 = array_filter($map3, 'is_string', ARRAY_FILTER_USE_BOTH);
	assertType('array<int|string, string>', $filtered3);
}

/**
 * @param array<int, int> $list1
 */
function nonCallableStringIsIgnored(array $list1): void
{
	$filtered1 = array_filter($list1, 'foo');
	assertType('array<int, int>', $filtered1);
}

/**
 * @param array<int|string, int|string> $map1
 * @param array<int|string, int|string> $map2
 */
function nonBuiltInFunctionsAreNotSupportedYetAndThereforeIgnored(array $map1, array $map2): void
{
	$filtered1 = array_filter($map1, '\ArrayFilter\isString');
	assertType('array<int|string, int|string>', $filtered1);

	$filtered2 = array_filter($map2, '\ArrayFilter\Filters::isString');
	assertType('array<int|string, int|string>', $filtered2);
}

/**
 * @param mixed $value
 */
function isString($value): bool
{
	return is_string($value);
}

class Filters {
	/**
	 * @param mixed $value
	 */
	public static function isString($value): bool
	{
		return is_string($value);
	}
}

function unionOfCallableStrings(): void
{
	$func = rand(0, 1) === 1 ? 'is_string' : 'is_int';
	$list = [
		1,
		2,
		'foo',
	];
	assertType("array{1, 2}|array{2: 'foo'}", array_filter($list, $func)); // SKIP: array functions flatten the shapes they are given
}
