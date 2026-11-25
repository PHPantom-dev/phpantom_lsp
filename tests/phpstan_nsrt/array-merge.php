<?php

namespace ArrayMerge;

use function array_merge;
use function PHPStan\Testing\assertType;

function foo(): void
{
	$foo = ['foo' => 17, 'a', 'bar' => 18, 'b'];
	$bar = [99 => 'b', 'bar' => 19, 98 => 'c'];
	$baz = array_merge($foo, $bar);

	assertType('array{foo: 17, 0: \'a\', bar: 19, 1: \'b\', 2: \'b\', 3: \'c\'}', $baz);
}

/**
 * @param string[][] $foo
 * @param array<non-empty-array<int>> $bar1
 * @param non-empty-array<non-empty-array<int>> $bar2
 * @param non-empty-array<array<int>> $bar3
 */
function unpackingArrays(array $foo, array $bar1, array $bar2, array $bar3): void
{
	assertType('array<string>', array_merge([], ...$foo));
	assertType('array<int>', array_merge([], ...$bar1));
	assertType('array<int>', array_merge([], ...$bar3));
}

function unpackingConstantArrays(): void
{
	assertType('array{}', array_merge([], ...[]));
	assertType('array{17}', array_merge([], [17]));
	assertType('array{17}', array_merge([], ...[[17]]));
	assertType('array{foo: \'bar\', bar: \'baz2\', 0: 17}', array_merge(['foo' => 'bar', 'bar' => 'baz1'], ['bar' => 'baz2', 17]));
	assertType('array{foo: \'bar\', bar: \'baz2\', 0: 17}', array_merge(['foo' => 'bar', 'bar' => 'baz1'], ...[['bar' => 'baz2', 17]]));
}

/**
 * @param list<string> $a
 * @param list<string> $b
 * @return void
 */
function listIsStillList(array $a, array $b): void
{
	assertType('list<string>', array_merge($a, $b));

	$c = [];
	foreach ($a as $v) {
		$c = array_merge($a, $c);
	}
	assertType('list<string>', $c);
}
