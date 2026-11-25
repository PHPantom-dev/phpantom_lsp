<?php

namespace ListType;

use function PHPStan\Testing\assertType;

class Foo
{
	/** @param list $list */
	public function directAssertion($list): void
	{
		assertType('list', $list);
	}

	/** @param list $list */
	public function directAssertionParamHint(array $list): void
	{
		assertType('list', $list);
	}

	/** @param list $list */
	public function directAssertionNullableParamHint(array $list = null): void
	{
		assertType('list|null', $list);
	}

	/** @param list<\DateTime> $list */
	public function directAssertionObjectParamHint($list): void
	{
		assertType('list<DateTime>', $list);
	}

	public function withoutGenerics(): void
	{
		/** @var list $list */
		$list = [];
		$list[] = '1';
		$list[] = true;
		$list[] = new \stdClass();
		assertType('non-empty-list<\'1\'|true|stdClass>', $list); // PHPStan keeps the declared element type; PHPantom is more precise here
	}


	public function withMixedType(): void
	{
		/** @var list<mixed> $list */
		$list = [];
		$list[] = '1';
		$list[] = true;
		$list[] = new \stdClass();
		assertType('non-empty-list<mixed>', $list);
	}

	public function withObjectType(): void
	{
		/** @var list<\DateTime> $list */
		$list = [];
		$list[] = new \DateTime();
		assertType('non-empty-list<DateTime>', $list);
	}

	/** @return list<scalar> */
	public function withScalarGoodContent(): void
	{
		/** @var list<scalar> $list */
		$list = [];
		$list[] = '1';
		$list[] = true;
		assertType('non-empty-list<scalar>', $list); // `scalar` is `bool|float|int|string`
	}

	public function withNumericKey(): void
	{
		/** @var list $list */
		$list = [];
		$list[] = '1';
		$list['1'] = true;
	}

	public function withFullListFunctionality(): void
	{
		// These won't output errors for now but should when list type will be fully implemented
		/** @var list $list */
		$list = [];
		assertType('list', $list);
		$list[] = '1';
		assertType('non-empty-list<\'1\'>', $list); // PHPStan keeps the declared element type; PHPantom is more precise here
		$list[] = '2';
		assertType('non-empty-list<\'1\'|\'2\'>', $list); // PHPStan keeps the declared element type; PHPantom is more precise here
		unset($list[0]);//break list behaviour
		assertType('array<int, \'1\'|\'2\'>', $list); // PHPStan keeps the declared element type; PHPantom is more precise here

		/** @var list $list2 */
		$list2 = [];
		assertType('list', $list2);
		$list2[2] = '1';//Most likely to create a gap in indexes
	}

	/** @param list<int> $list */
	public function testUnset(array $list): void
	{
		assertType('list<int>', $list);
		unset($list[2]);
	}

	/** @param list<int> $list */
	public function testSetOffsetExplicitlyWithoutGap(array $list): void
	{
		assertType('list<int>', $list);
		$list[0] = 17;
		$list[1] = 19;
		$list[0] = 21;
	}

	/** @param list<int> $list */
	public function testSetOffsetExplicitlyWithGap(array $list): void
	{
		assertType('list<int>', $list);
		$list[0] = 17;
		$list[2] = 21;
	}

	/** @param list<int> $list */
	function testAppendImmediatelyAfterLastElement(array $list): void
	{
		assertType('list<int>', $list);
		$list[0] = 17;
		$list[1] = 19;
		$list[2] = 21;
		$list[3] = 21;

		// hole in the list -> turns it into a array

		$list[5] = 21;
	}


	/** @param list<int> $list */
	function testKeepListAfterLast(array $list): void
	{
		if (isset($list[5])) {
			$list[6] = 21;
		}
		assertType('list<int>', $list); // SKIP: writing the offset after a known one turns a list into an array
	}

	/** @param list<int> $list */
	function testKeepListAfterLastArrayKey(array $list): void
	{
		if (array_key_exists(5, $list) && is_int($list[5])) {
			$list[6] = 21;
		}
		assertType('list<int>', $list); // SKIP: writing the offset after a known one turns a list into an array
	}
}
