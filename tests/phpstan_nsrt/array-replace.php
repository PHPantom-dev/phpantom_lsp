<?php

namespace NonEmptyArrayReplace;

use function PHPStan\Testing\assertType;

class Foo
{
	/**
	 * @param array $array1
	 * @param array $array2
	 */
	public function arrayReplace($array1, $array2): void
	{
		assertType("array", array_replace($array1, []));
		assertType("array", array_replace([], $array1));
		assertType("array", array_replace($array1, $array2));
	}

	/**
	 * @param array{foo: '1', bar: '2'} $array1
	 * @param array{foo: '1', bar: '4'} $array2
	 */
	public function arrayReplaceArrayShapes($array1, $array2): void
	{
		assertType("array{foo: '1', bar: '2'}", array_replace($array1));
		assertType("array{foo: '1', bar: '2'}", array_replace([], $array1));
		assertType("array{foo: '1', bar: '4'}", array_replace($array1, $array2));
	}

	/**
	 * @param int[] $array1
	 * @param string[] $array2
	 */
	public function arrayReplaceSimple($array1, $array2): void
	{
		assertType("array<int>", array_replace($array1, $array1));
		assertType("array<int|string>", array_replace($array1, $array2));
		assertType("array<int|string>", array_replace($array2, $array1));
	}

	/**
	 * @param int[] ...$arrays1
	 */
	public function arrayReplaceVariadic(...$arrays1): void
	{
		assertType("array<int>", array_replace(...$arrays1));
	}

	/**
	 * @param array<int, int|string> $array1
	 * @param array<int, bool|float> $array2
	 */
	public function arrayReplaceUnionType($array1, $array2): void
	{
		assertType("array<int, int|string>", array_replace($array1, $array1));
		assertType("array<int, bool|float|int|string>", array_replace($array1, $array2));
		assertType("array<int, bool|float|int|string>", array_replace($array2, $array1));
	}

	/**
	 * @param array<int, array{bar: '2'}|array{foo: '1'}> $array1
	 * @param array<int, array{bar: '3'}|array{foo: '2'}> $array2
	 */
	public function arrayReplaceUnionTypeArrayShapes($array1, $array2): void
	{
		assertType("array<int, array{bar: '2'}|array{foo: '1'}>", array_replace($array1, $array1));
		assertType("array<int, array{bar: '2'}|array{bar: '3'}|array{foo: '1'}|array{foo: '2'}>", array_replace($array1, $array2));
		assertType("array<int, array{bar: '2'}|array{bar: '3'}|array{foo: '1'}|array{foo: '2'}>", array_replace($array2, $array1));
	}

	/**
	 * @param array{foo: '1', bar: '2'} $array1
	 * @param array<string, int> $array2
	 * @param array<int, string> $array3
	 */
	public function arrayReplaceArrayShapeAndGeneralArray($array1, $array2, $array3): void
	{
		assertType("non-empty-array<string, '1'|'2'|int>&hasOffset('bar')&hasOffset('foo')", array_replace($array1, $array2)); // SKIP: a shape replaced with a generic array does not keep the shape's keys
		assertType("non-empty-array<string, '1'|'2'|int>&hasOffsetValue('bar', '2')&hasOffsetValue('foo', '1')", array_replace($array2, $array1)); // SKIP: a shape replaced with a generic array does not keep the shape's keys

		assertType("non-empty-array<'bar'|'foo'|int, string>&hasOffset('bar')&hasOffset('foo')", array_replace($array1, $array3)); // SKIP: a shape replaced with a generic array does not keep the shape's keys
		assertType("non-empty-array<'bar'|'foo'|int, string>&hasOffsetValue('bar', '2')&hasOffsetValue('foo', '1')", array_replace($array3, $array1)); // SKIP: a shape replaced with a generic array does not keep the shape's keys

		assertType("array<int|string, int|string>", array_replace($array2, $array3));
	}

	/**
	 * @param array{0: 1, 1: 2} $array1
	 * @param array{1: 3, 2: 4} $array2
	 */
	public function arrayReplaceNumericKeys($array1, $array2): void
	{
		assertType("array{1, 3, 4}", array_replace($array1, $array2));
	}

	/**
	 * @param list<int> $array1
	 * @param list<int> $array2
	 */
	public function arrayReplaceLists($array1, $array2): void
	{
		assertType("list<int>", array_replace($array1, $array2));
	}
}
