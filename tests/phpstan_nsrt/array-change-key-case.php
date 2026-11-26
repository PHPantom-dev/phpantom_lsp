<?php declare(strict_types = 1);

namespace ArrayChangeKeyCase;

use function PHPStan\Testing\assertType;

class HelloWorld
{
	/**
	 * @param array<string> $arr1
	 * @param array<string, string> $arr2
	 * @param array<string|int, string> $arr3
	 * @param array<int, string> $arr4
	 * @param array<lowercase-string, string> $arr5
	 * @param array<lowercase-string&non-falsy-string, string> $arr6
	 * @param array<non-empty-string, string> $arr7
	 * @param array<literal-string, string> $arr8
	 * @param array{foo: 1, bar?: 2} $arr9
	 * @param array<'foo'|'bar', string> $arr10
	 * @param list<string> $list
	 * @param non-empty-array<string> $nonEmpty
	 */
	public function sayHello(
		array $arr1,
		array $arr2,
		array $arr3,
		array $arr4,
		array $arr5,
		array $arr6,
		array $arr7,
		array $arr8,
		array $arr9,
		array $arr10,
		array $list,
		array $nonEmpty,
		int $case
	): void {
		assertType('array<string>', array_change_key_case($arr1));
		assertType('array<string>', array_change_key_case($arr1, CASE_LOWER));
		assertType('array<string>', array_change_key_case($arr1, CASE_UPPER));
		assertType('array<string>', array_change_key_case($arr1, $case));

		assertType('array<lowercase-string, string>', array_change_key_case($arr2));
		assertType('array<lowercase-string, string>', array_change_key_case($arr2, CASE_LOWER));
		assertType('array<uppercase-string, string>', array_change_key_case($arr2, CASE_UPPER));
		assertType('array<string, string>', array_change_key_case($arr2, $case));

		assertType('array<int|lowercase-string, string>', array_change_key_case($arr3));
		assertType('array<int|lowercase-string, string>', array_change_key_case($arr3, CASE_LOWER));
		assertType('array<int|uppercase-string, string>', array_change_key_case($arr3, CASE_UPPER));
		assertType('array<int|string, string>', array_change_key_case($arr3, $case));

		assertType('array<int, string>', array_change_key_case($arr4));
		assertType('array<int, string>', array_change_key_case($arr4, CASE_LOWER));
		assertType('array<int, string>', array_change_key_case($arr4, CASE_UPPER));
		assertType('array<int, string>', array_change_key_case($arr4, $case));

		assertType('array<lowercase-string, string>', array_change_key_case($arr5));
		assertType('array<lowercase-string, string>', array_change_key_case($arr5, CASE_LOWER));
		assertType('array<uppercase-string, string>', array_change_key_case($arr5, CASE_UPPER));
		assertType('array<string, string>', array_change_key_case($arr5, $case));

		assertType('array<lowercase-string&non-falsy-string, string>', array_change_key_case($arr6));
		assertType('array<lowercase-string&non-falsy-string, string>', array_change_key_case($arr6, CASE_LOWER));
		assertType('array<non-falsy-string&uppercase-string, string>', array_change_key_case($arr6, CASE_UPPER));
		assertType('array<non-falsy-string, string>', array_change_key_case($arr6, $case));

		assertType('array<lowercase-string&non-empty-string, string>', array_change_key_case($arr7));
		assertType('array<lowercase-string&non-empty-string, string>', array_change_key_case($arr7, CASE_LOWER));
		assertType('array<non-empty-string&uppercase-string, string>', array_change_key_case($arr7, CASE_UPPER));
		assertType('array<non-empty-string, string>', array_change_key_case($arr7, $case));

		assertType('array<lowercase-string, string>', array_change_key_case($arr8));
		assertType('array<lowercase-string, string>', array_change_key_case($arr8, CASE_LOWER));
		assertType('array<uppercase-string, string>', array_change_key_case($arr8, CASE_UPPER));
		assertType('array<string, string>', array_change_key_case($arr8, $case));

		assertType('array{foo: 1, bar?: 2}', array_change_key_case($arr9));
		assertType('array{foo: 1, bar?: 2}', array_change_key_case($arr9, CASE_LOWER));
		assertType('array{FOO: 1, BAR?: 2}', array_change_key_case($arr9, CASE_UPPER));
		assertType("non-empty-array<'BAR'|'bar'|'FOO'|'foo', 1|2>", array_change_key_case($arr9, $case));

		assertType("array<'bar'|'foo', string>", array_change_key_case($arr10));
		assertType("array<'bar'|'foo', string>", array_change_key_case($arr10, CASE_LOWER));
		assertType("array<'BAR'|'FOO', string>", array_change_key_case($arr10, CASE_UPPER));
		assertType("array<'BAR'|'bar'|'FOO'|'foo', string>", array_change_key_case($arr10, $case));

		assertType('list<string>', array_change_key_case($list));
		assertType('list<string>', array_change_key_case($list, CASE_LOWER));
		assertType('list<string>', array_change_key_case($list, CASE_UPPER));
		assertType('list<string>', array_change_key_case($list, $case));

		assertType('non-empty-array<string>', array_change_key_case($nonEmpty));
		assertType('non-empty-array<string>', array_change_key_case($nonEmpty, CASE_LOWER));
		assertType('non-empty-array<string>', array_change_key_case($nonEmpty, CASE_UPPER));
		assertType('non-empty-array<string>', array_change_key_case($nonEmpty, $case));
	}
}
