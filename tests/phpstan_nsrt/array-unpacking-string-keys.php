<?php // lint >= 8.1

namespace ArrayUnpackingWithStringKeys;

use function PHPStan\Testing\assertType;

$foo = ['a' => 0, ...['a' => 1], ...['b' => 2]];

assertType('array{a: 1, b: 2}', $foo);

$bar = [1, ...['a' => 1], ...['b' => 2]];

assertType('array{0: 1, a: 1, b: 2}', $bar);

/**
 * @param array<string, int> $a
 * @param array<int, int> $b
 */
function foo(array $a, array $b)
{
	$c = [...$a, ...$b];

	assertType('array<int|string, int>', $c);
}

/**
 * @param array<array-key, int> $a
 * @param array<int, int> $b
 */
function bar(array $a, array $b)
{
	$c = [...$a, ...$b];

	assertType('array<int>', $c);
}

/**
 * @param array<string, int> $a
 * @param array<string, int> $b
 */
function baz(array $a, array $b)
{
	$c = [...$a, ...$b];

	assertType('array<string, int>', $c);
}

/**
 * @param non-empty-array<string, int> $a
 * @param array<int, int> $b
 */
function nonEmptyArray1(array $a, array $b)
{
	$c = [...$a, ...$b];

}

/**
 * @param array<string, int> $a
 * @param non-empty-array<int, int> $b
 */
function nonEmptyArray2(array $a, array $b)
{
	$c = [...$a, ...$b];

}
