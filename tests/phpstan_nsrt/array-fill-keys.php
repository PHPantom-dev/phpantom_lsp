<?php

namespace ArrayFillKeys;

use function PHPStan\Testing\assertType;

class Foo
{
	/** @phpstan-return 'foo' */
	public function __toString(): string
	{
		return 'foo';
	}
}

class Bar
{
	public function __toString(): string
	{
		return 'bar';
	}
}

class Baz {}

function withBoolKey() : array
{
	assertType("array{1: 'b'}", array_fill_keys([true], 'b'));
	assertType("array{'': 'b'}", array_fill_keys([false], 'b'));
}

function withFloatKey() : array
{
	assertType("array{'1.5': 'b'}", array_fill_keys([1.5], 'b'));
}

function withIntegerKey() : array
{
	assertType("array{99: 'b'}", array_fill_keys([99], 'b'));
}

function withNumericStringKey() : array
{
	assertType("array{999: 'b'}", array_fill_keys(["999"], 'b'));
}

function withObjectKey() : array
{
	assertType("array{foo: 'b'}", array_fill_keys([new Foo()], 'b')); // SKIP: array_fill_keys() does not read an object key's __toString()
	assertType("non-empty-array<string, 'b'>", array_fill_keys([new Bar()], 'b')); // SKIP: array_fill_keys() does not read an object key's __toString()
	assertType("*ERROR*", array_fill_keys([new Baz()], 'b')); // SKIP: a call PHP rejects is not reported as an error type
}

function withUnionKeys(): void
{
	$arr1 = ['foo', rand(0, 1) ? 'bar1' : 'bar2', 'baz'];
	assertType("array{foo: 'b', bar1?: 'b', bar2?: 'b', baz: 'b'}", array_fill_keys($arr1, 'b'));

	$arr2 = ['foo'];
	if (rand(0, 1)) {
		$arr2[] = 'bar';
	}
	$arr2[] = 'baz';
	assertType("array{foo: 'b', bar?: 'b', baz?: 'b'}", array_fill_keys($arr2, 'b'));
}

function withOptionalKeys(): void
{
	$arr1 = ['foo', 'bar'];
	if (rand(0, 1)) {
		$arr1[] = 'baz';
	}
	assertType("array{foo: 'b', bar: 'b'}|array{foo: 'b', bar: 'b', baz: 'b'}", array_fill_keys($arr1, 'b')); // PHPStan joins the two shapes into `array{foo: 'b', bar: 'b', baz?: 'b'}`

	/** @var array{0?: 'foo', 1: 'bar', }|array{0: 'baz', 1?: 'foobar'} $arr2 */
	$arr2 = [];
	assertType("array{baz: 'b', foobar?: 'b'}|array{foo?: 'b', bar: 'b'}", array_fill_keys($arr2, 'b'));
}

/**
 * @param Bar[] $foo
 * @param int[] $bar
 * @param Foo[] $baz
 * @param float[] $floats
 * @param array<int, int|string|bool> $mixed
 * @param list<string> $list
 * @param Baz[] $objectsWithoutToString
 */
function withNotConstantArray(array $foo, array $bar, array $baz, array $floats, array $mixed, array $list, array $objectsWithoutToString): void
{
	assertType("array<string, null>", array_fill_keys($foo, null)); // SKIP: array_fill_keys() does not read an object key's __toString()
	assertType("array<int, null>", array_fill_keys($bar, null));
	assertType("array<'foo', null>", array_fill_keys($baz, null)); // SKIP: array_fill_keys() does not read an object key's __toString()
	assertType("array<numeric-string&uppercase-string, null>", array_fill_keys($floats, null));
	assertType("array<int|string, null>", array_fill_keys($mixed, null)); // PHPStan says `bool|int|string`, but `true` is stored as `1` and `false` as `''`
	assertType('array<string, null>', array_fill_keys($list, null));
	assertType('*ERROR*', array_fill_keys($objectsWithoutToString, null)); // SKIP: a call PHP rejects is not reported as an error type

	if (array_key_exists(17, $mixed)) {
		assertType('non-empty-array<int|string, null>', array_fill_keys($mixed, null)); // PHPStan says `bool|int|string`, but `true` is stored as `1` and `false` as `''`
	}

	if (array_key_exists(17, $mixed) && $mixed[17] === 'foo') {
		assertType('non-empty-array<int|string, null>', array_fill_keys($mixed, null)); // PHPStan says `bool|int|string`, but `true` is stored as `1` and `false` as `''`
	}
}
