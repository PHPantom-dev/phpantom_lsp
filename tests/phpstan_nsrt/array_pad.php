<?php

namespace ArrayPad;

use function PHPStan\Testing\assertType;

class HelloWorld
{
	/**
	 * @param array<string, string> 	      $arrayString
	 * @param array<int, int>       		  $arrayInt
	 * @param non-empty-array<string, string> $nonEmptyArrayString
	 * @param non-empty-array<int, int>       $nonEmptyArrayInt
	 * @param list<string> 					  $listString
	 * @param list<int>       				  $listInt
	 * @param non-empty-list<string> 		  $nonEmptyListString
	 * @param non-empty-list<int>             $nonEmptyListInt
	 * @param int							  $int
	 * @param positive-int					  $positiveInt
	 * @param negative-int					  $negativeInt
	 * @param positive-int|negative-int		  $nonZero
	 */
	public function test(
		$arrayString,
		$arrayInt,
		$nonEmptyArrayString,
		$nonEmptyArrayInt,
		$listString,
		$listInt,
		$nonEmptyListString,
		$nonEmptyListInt,
		$int,
		$positiveInt,
		$negativeInt,
		$nonZero,
	): void
	{
		assertType('array<int|string, string>', array_pad($arrayString, $int, 'foo'));
		assertType('non-empty-array<int|string, string>', array_pad($arrayString, $positiveInt, 'foo'));
		assertType('non-empty-array<int|string, string>', array_pad($arrayString, $negativeInt, 'foo'));
		assertType('non-empty-array<int|string, string>', array_pad($arrayString, $nonZero, 'foo'));

		assertType('array<int, \'foo\'|int>', array_pad($arrayInt, $int, 'foo'));
		assertType('non-empty-array<int, \'foo\'|int>', array_pad($arrayInt, $positiveInt, 'foo'));
		assertType('non-empty-array<int, \'foo\'|int>', array_pad($arrayInt, $negativeInt, 'foo'));
		assertType('non-empty-array<int, \'foo\'|int>', array_pad($arrayInt, $nonZero, 'foo'));

		assertType('non-empty-array<int|string, string>', array_pad($nonEmptyArrayString, $int, 'foo'));
		assertType('non-empty-array<int|string, string>', array_pad($nonEmptyArrayString, $positiveInt, 'foo'));
		assertType('non-empty-array<int|string, string>', array_pad($nonEmptyArrayString, $negativeInt, 'foo'));
		assertType('non-empty-array<int|string, string>', array_pad($nonEmptyArrayString, $nonZero, 'foo'));

		assertType('non-empty-array<int, \'foo\'|int>', array_pad($nonEmptyArrayInt, $int, 'foo'));
		assertType('non-empty-array<int, \'foo\'|int>', array_pad($nonEmptyArrayInt, $positiveInt, 'foo'));
		assertType('non-empty-array<int, \'foo\'|int>', array_pad($nonEmptyArrayInt, $negativeInt, 'foo'));
		assertType('non-empty-array<int, \'foo\'|int>', array_pad($nonEmptyArrayInt, $nonZero, 'foo'));

		assertType('list<string>', array_pad($listString, $int, 'foo'));
		assertType('non-empty-list<string>', array_pad($listString, $positiveInt, 'foo'));
		assertType('non-empty-list<string>', array_pad($listString, $negativeInt, 'foo'));
		assertType('non-empty-list<string>', array_pad($listString, $nonZero, 'foo'));

		assertType('list<\'foo\'|int>', array_pad($listInt, $int, 'foo'));
		assertType('non-empty-list<\'foo\'|int>', array_pad($listInt, $positiveInt, 'foo'));
		assertType('non-empty-list<\'foo\'|int>', array_pad($listInt, $negativeInt, 'foo'));
		assertType('non-empty-list<\'foo\'|int>', array_pad($listInt, $nonZero, 'foo'));

		assertType('non-empty-list<string>', array_pad($nonEmptyListString, $int, 'foo'));
		assertType('non-empty-list<string>', array_pad($nonEmptyListString, $positiveInt, 'foo'));
		assertType('non-empty-list<string>', array_pad($nonEmptyListString, $negativeInt, 'foo'));
		assertType('non-empty-list<string>', array_pad($nonEmptyListString, $nonZero, 'foo'));

		assertType('non-empty-list<\'foo\'|int>', array_pad($nonEmptyListInt, $int, 'foo'));
		assertType('non-empty-list<\'foo\'|int>', array_pad($nonEmptyListInt, $positiveInt, 'foo'));
		assertType('non-empty-list<\'foo\'|int>', array_pad($nonEmptyListInt, $negativeInt, 'foo'));
		assertType('non-empty-list<\'foo\'|int>', array_pad($nonEmptyListInt, $nonZero, 'foo'));
	}
}
