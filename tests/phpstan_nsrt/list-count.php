<?php

namespace ListCount;

use function PHPStan\Testing\assertType;

/**
 * @param list<int> $items
 */
function foo(array $items) {
	assertType('list<int>', $items);
	if (count($items) === 3) {
		assertType('array{int, int, int}', $items);
		array_shift($items);
		assertType('array{int, int}', $items);
	} elseif (count($items) === 0) {
		assertType('array{}', $items);
	} elseif (count($items) === 5) {
		assertType('array{int, int, int, int, int}', $items);
	} else {
		assertType('non-empty-list<int>', $items);
	}
	assertType('list<int>', $items);
}

/**
 * @param list<int> $items
 */
function modeCount(array $items, int $mode) {
	assertType('list<int>', $items);
	if (count($items, $mode) === 3) {
		assertType('array{int, int, int}', $items);
		array_shift($items);
		assertType('array{int, int}', $items);
	} elseif (count($items, $mode) === 0) {
		assertType('array{}', $items);
	} elseif (count($items, $mode) === 5) {
		assertType('array{int, int, int, int, int}', $items);
	} else {
		assertType('non-empty-list<int>', $items);
	}
	assertType('list<int>', $items);
}

/**
 * @param list<int|int[]> $items
 */
function modeCountOnMaybeArray(array $items, int $mode) {
	assertType('list<array<int>|int>', $items);
	if (count($items, $mode) === 3) {
		array_shift($items);
		assertType('list<array<int>|int>', $items);
	} elseif (count($items, $mode) === 0) {
		assertType('array{}', $items);
	} elseif (count($items, $mode) === 5) {
		assertType('non-empty-list<array<int>|int>', $items);
	} else {
		assertType('non-empty-list<array<int>|int>', $items);
	}
	assertType('list<array<int>|int>', $items);
}


/**
 * @param list<int> $items
 */
function normalCount(array $items) {
	assertType('list<int>', $items);
	if (count($items, COUNT_NORMAL) === 3) {
		assertType('array{int, int, int}', $items);
		array_shift($items);
		assertType('array{int, int}', $items);
	} elseif (count($items, COUNT_NORMAL) === 0) {
		assertType('array{}', $items);
	} elseif (count($items, COUNT_NORMAL) === 5) {
		assertType('array{int, int, int, int, int}', $items);
	} else {
		assertType('non-empty-list<int>', $items);
	}
	assertType('list<int>', $items);
}

/**
 * @param list<int|int[]> $items
 */
function recursiveCountOnMaybeArray(array $items):void {
	assertType('list<array<int>|int>', $items);
	if (count($items, COUNT_RECURSIVE) === 3) {
		array_shift($items);
		assertType('list<array<int>|int>', $items);
	} elseif (count($items, COUNT_RECURSIVE) === 0) {
		assertType('array{}', $items);
	} elseif (count($items, COUNT_RECURSIVE) === 5) {
		assertType('non-empty-list<array<int>|int>', $items);
	} else {
		assertType('non-empty-list<array<int>|int>', $items);
	}
	assertType('list<array<int>|int>', $items);
}

/**
 * @param list<int|int[]> $items
 */
function normalCountOnMaybeArray(array $items):void {
	assertType('list<array<int>|int>', $items);
	if (count($items, COUNT_NORMAL) === 3) {
		assertType('array{array<int>|int, array<int>|int, array<int>|int}', $items);
		array_shift($items);
		assertType('array{array<int>|int, array<int>|int}', $items);
	} elseif (count($items, COUNT_NORMAL) === 0) {
		assertType('array{}', $items);
	} elseif (count($items, COUNT_NORMAL) === 5) {
		assertType('array{array<int>|int, array<int>|int, array<int>|int, array<int>|int, array<int>|int}', $items);
	} else {
		assertType('non-empty-list<array<int>|int>', $items);
	}
	assertType('list<array<int>|int>', $items);
}

class A {}

/**
 * @param list<A> $items
 */
function cannotCountRecursive($items, int $mode)
{
	if (count($items) === 3) {
		assertType('array{ListCount\A, ListCount\A, ListCount\A}', $items);
	}
	if (count($items, COUNT_NORMAL) === 3) {
		assertType('array{ListCount\A, ListCount\A, ListCount\A}', $items);
	}
	if (count($items, COUNT_RECURSIVE) === 3) {
		assertType('array{ListCount\A, ListCount\A, ListCount\A}', $items);
	}
	if (count($items, $mode) === 3) {
		assertType('array{ListCount\A, ListCount\A, ListCount\A}', $items);
	}
}

/**
 * @param list<array<A>> $items
 */
function cannotCountRecursiveNestedArray($items, int $mode)
{
	if (count($items) === 3) {
		assertType('array{array<ListCount\A>, array<ListCount\A>, array<ListCount\A>}', $items);
	}
	if (count($items, COUNT_NORMAL) === 3) {
		assertType('array{array<ListCount\A>, array<ListCount\A>, array<ListCount\A>}', $items);
	}
	if (count($items, COUNT_RECURSIVE) === 3) {
	}
	if (count($items, $mode) === 3) {
	}
}

class CountableFoo implements \Countable
{
	public function count(): int
	{
		return 3;
	}
}

/**
 * @param list<CountableFoo> $items
 */
function cannotCountRecursiveCountable($items, int $mode)
{
	if (count($items) === 3) {
		assertType('array{ListCount\CountableFoo, ListCount\CountableFoo, ListCount\CountableFoo}', $items);
	}
	if (count($items, COUNT_NORMAL) === 3) {
		assertType('array{ListCount\CountableFoo, ListCount\CountableFoo, ListCount\CountableFoo}', $items);
	}
	if (count($items, COUNT_RECURSIVE) === 3) {
		assertType('array{ListCount\CountableFoo, ListCount\CountableFoo, ListCount\CountableFoo}', $items);
	}
	if (count($items, $mode) === 3) {
		assertType('array{ListCount\CountableFoo, ListCount\CountableFoo, ListCount\CountableFoo}', $items);
	}
}

function countCountable(CountableFoo $x, int $mode)
{
	if (count($x) === 3) {
		assertType('ListCount\CountableFoo', $x);
	} else {
		assertType('ListCount\CountableFoo', $x);
	}
	assertType('ListCount\CountableFoo', $x);

	if (count($x, COUNT_NORMAL) === 3) {
		assertType('ListCount\CountableFoo', $x);
	} else {
		assertType('ListCount\CountableFoo', $x);
	}
	assertType('ListCount\CountableFoo', $x);

	if (count($x, COUNT_RECURSIVE) === 3) {
		assertType('ListCount\CountableFoo', $x);
	} else {
		assertType('ListCount\CountableFoo', $x);
	}
	assertType('ListCount\CountableFoo', $x);

	if (count($x, $mode) === 3) {
		assertType('ListCount\CountableFoo', $x);
	} else {
		assertType('ListCount\CountableFoo', $x);
	}
	assertType('ListCount\CountableFoo', $x);
}

class CountWithOptionalKeys
{
	/**
	 * @param array{0: mixed, 1?: string|null} $row
	 */
	protected function testOptionalKeys($row): void
	{
		if (count($row) === 0) {
			assertType('*NEVER*', $row);
		} else {
			assertType('array{0: mixed, 1?: string|null}', $row);
		}

		if (count($row) === 1) {
			assertType('array{mixed}', $row);
		} else {
			assertType('array{mixed, string|null}', $row);
		}

		if (count($row) === 2) {
			assertType('array{mixed, string|null}', $row);
		} else {
			assertType('array{mixed}', $row);
		}

		if (count($row) === 3) {
			assertType('*NEVER*', $row);
		} else {
			assertType('array{0: mixed, 1?: string|null}', $row);
		}
	}

	/**
	 * @param array{mixed}|array{0: mixed, 1?: string|null} $row
	 */
	protected function testOptionalKeysInUnion($row): void
	{
		if (count($row) === 0) {
			assertType('*NEVER*', $row);
		} else {
			assertType('array{0: mixed, 1?: string|null}', $row);
		}

		if (count($row) === 1) {
			assertType('array{mixed}', $row);
		} else {
			assertType('array{mixed, string|null}', $row);
		}

		if (count($row) === 2) {
			assertType('array{mixed, string|null}', $row);
		} else {
			assertType('array{mixed}', $row);
		}
		// The branches above re-fold into the original shape rather than
		// staying two alternatives: the positional `array{mixed}` an
		// unmatched count leaves behind and the keyed `array{0: mixed,
		// 1?: string|null}` a matched count narrows agree on the value at
		// position 0, so the explicit key anchors the join.
		assertType('array{0: mixed, 1?: string|null}', $row);

		if (count($row) === 3) {
			assertType('*NEVER*', $row);
		} else {
			assertType('array{0: mixed, 1?: string|null}', $row);
		}
	}

	/**
	 * @param array{string}|array{0: int, 1?: string|null} $row
	 */
	protected function testOptionalKeysInListsOfTaggedUnion($row): void
	{
		if (count($row) === 0) {
			assertType('*NEVER*', $row);
		} else {
			assertType('array{0: int, 1?: string|null}|array{string}', $row);
		}

		if (count($row) === 1) {
			assertType('array{int}|array{string}', $row); // PHPStan keeps the optional entry, which the count proves absent
		} else {
			assertType('array{int, string|null}', $row);
		}

		if (count($row) === 2) {
			assertType('array{int, string|null}', $row);
		} else {
			assertType('array{int}|array{string}', $row); // PHPStan keeps the optional entry, which the count proves absent
		}

		if (count($row) === 3) {
			assertType('*NEVER*', $row);
		} else {
			assertType('array{0: int, 1?: string|null}|array{string}', $row);
		}
	}

	/**
	 * A positional shape one branch produces can join with a keyed shape
	 * another branch produces when the two agree on the value at every
	 * position they share: the explicit key is then just a longer
	 * spelling of the same position, not a different tag.
	 */
	protected function testPositionalShapeJoinsKeyedShapeWhenValuesAgree(bool $flag): void
	{
		if ($flag) {
			$row = ['a'];
		} else {
			$row = [0 => 'a', 1 => 'b'];
		}
		assertType("array{'a', 1?: 'b'}", $row);
	}

	/**
	 * A positional shape must not join with a keyed shape when the two
	 * disagree on the value at a shared position: that is not one shape
	 * narrowed two ways, it is two differently-tagged alternatives that
	 * merely happen to share a length.
	 */
	protected function testPositionalShapeDoesNotJoinKeyedShapeWhenValuesDisagree(bool $flag): void
	{
		if ($flag) {
			$row = ['a'];
		} else {
			$row = [0 => 1, 1 => 'b'];
		}
		assertType("array{'a'}|array{0: 1, 1: 'b'}", $row);
	}

	/**
	 * @param array{string}|array{0: int, 3?: string|null} $row
	 */
	protected function testOptionalKeysInUnionArray($row): void
	{
		if (count($row) === 0) {
			assertType('*NEVER*', $row);
		} else {
			assertType('array{0: int, 3?: string|null}|array{string}', $row);
		}

		if (count($row) === 1) {
			assertType('array{int}|array{string}', $row); // PHPStan keeps the optional entry, which the count proves absent
		} else {
			assertType('array{0: int, 3: string|null}', $row); // PHPStan keeps the `3?`, but the count proves the entry present
		}

		if (count($row) === 2) {
			assertType('array{0: int, 3: string|null}', $row); // PHPStan keeps the `3?`, but the count proves the entry present
		} else {
			assertType('array{int}|array{string}', $row); // PHPStan keeps the optional entry, which the count proves absent
		}

		if (count($row) === 3) {
			assertType('*NEVER*', $row);
		} else {
			assertType('array{0: int, 3?: string|null}|array{string}', $row);
		}
	}

	/**
	 * @param array{string}|list{0: int, 1?: string|null, 2?: int|null, 3?: float|null} $row
	 * @param list<string> $listRow
	 * @param int<2, 3> $twoOrThree
	 * @param int<2, max> $twoOrMore
	 * @param int<min, 3> $maxThree
	 * @param int<10, 11> $tenOrEleven
	 * @param int<3, 32> $threeOrMoreInRangeLimit
	 * @param int<3, 512> $threeOrMoreOverRangeLimit
	 */
	protected function testOptionalKeysInUnionListWithIntRange($row, $listRow, $twoOrThree, $twoOrMore, int $maxThree, $tenOrEleven, $threeOrMoreInRangeLimit, $threeOrMoreOverRangeLimit): void
	{
		if (count($row) >= $twoOrThree) {
		} else {
			assertType('array{string}|list{0: int, 1?: string|null, 2?: int|null, 3?: float|null}', $row);
		}

		if (count($row) >= $tenOrEleven) {
		} else {
			assertType('array{string}|list{0: int, 1?: string|null, 2?: int|null, 3?: float|null}', $row);
		}

		if (count($row) >= $twoOrMore) {
		} else {
			assertType('array{string}|list{0: int, 1?: string|null, 2?: int|null, 3?: float|null}', $row);
		}

		if (count($row) >= $maxThree) {
			assertType('array{string}|list{0: int, 1?: string|null, 2?: int|null, 3?: float|null}', $row);
		} else {
			assertType('array{string}|list{0: int, 1?: string|null, 2?: int|null, 3?: float|null}', $row);
		}

		if (count($row) >= $threeOrMoreInRangeLimit) {
		} else {
			assertType('array{string}|list{0: int, 1?: string|null, 2?: int|null, 3?: float|null}', $row);
		}

		if (count($listRow) >= $threeOrMoreInRangeLimit) {
		} else {
			assertType('list<string>', $listRow);
		}

		if (count($row) >= $threeOrMoreOverRangeLimit) {
		} else {
			assertType('array{string}|list{0: int, 1?: string|null, 2?: int|null, 3?: float|null}', $row);
		}

		if (count($listRow) >= $threeOrMoreOverRangeLimit) {
		} else {
			assertType('list<string>', $listRow);
		}
	}

	/**
	 * @param array{string}|array{0: int, 1?: string|null, 2?: int|null, 3?: float|null} $row
	 * @param int<2, 3> $twoOrThree
	 */
	protected function testOptionalKeysInUnionArrayWithIntRange($row, $twoOrThree): void
	{
		if (count($row) >= $twoOrThree) {
		} else {
			assertType('array{0: int, 1?: string|null, 2?: int|null, 3?: float|null}|array{string}', $row);
		}
	}
}

class FooBug
{
	public int $totalExpectedRows = 0;

	/** @var list<\stdClass> */
	public array $importedDaySummaryRows = [];

	public function sayHello(): void
	{
		assertType('int', $this->totalExpectedRows);
		assertType('list<stdClass>', $this->importedDaySummaryRows);
		if ($this->totalExpectedRows !== count($this->importedDaySummaryRows)) {
			assertType('int', $this->totalExpectedRows);
			assertType('list<stdClass>', $this->importedDaySummaryRows);
		}
		assertType('int', $this->totalExpectedRows);
		assertType('list<stdClass>', $this->importedDaySummaryRows);
	}
}

class FooBugPositiveInt
{
	/**
	 * @var positive-int
	 */
	public int $totalExpectedRows = 1;

	/** @var list<\stdClass> */
	public array $importedDaySummaryRows = [];

	public function sayHello(): void
	{
		assertType('int<1, max>', $this->totalExpectedRows);
		assertType('list<stdClass>', $this->importedDaySummaryRows);
		if ($this->totalExpectedRows !== count($this->importedDaySummaryRows)) {
			assertType('int<1, max>', $this->totalExpectedRows);
			assertType('list<stdClass>', $this->importedDaySummaryRows);
		}
		assertType('int<1, max>', $this->totalExpectedRows);
		assertType('list<stdClass>', $this->importedDaySummaryRows);
	}
}
