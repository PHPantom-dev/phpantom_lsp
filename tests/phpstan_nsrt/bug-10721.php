<?php declare(strict_types = 1);

namespace Bug10721;

use function PHPStan\Testing\assertType;

final class HandpickedWordlistProvider
{
	/**
	 * @return non-empty-list<non-empty-string>
	 */
	public function retrieve(?int $limit = 20): array
	{
		$list = [
			'zib',
			'zib 2',
			'zeit im bild',
			'soko',
			'landkrimi',
			'tatort',
		];

		assertType("array{'zib', 'zib 2', 'zeit im bild', 'soko', 'landkrimi', 'tatort'}", $list);
		shuffle($list);
		assertType("non-empty-list<'landkrimi'|'soko'|'tatort'|'zeit im bild'|'zib'|'zib 2'>", $list);

		assertType("non-empty-list<'landkrimi'|'soko'|'tatort'|'zeit im bild'|'zib'|'zib 2'>", array_slice($list, 0, max($limit, 1)));
		return array_slice($list, 0, max($limit, 1));
	}

	public function listVariants(): void
	{
		$arr = [
			2 => 'zib',
			4 => 'zib 2',
		];

		assertType("array{2: 'zib', 4: 'zib 2'}", $arr);
		shuffle($arr);
		assertType("non-empty-list<'zib'|'zib 2'>", $arr);

		$list = [
			'zib',
			'zib 2',
		];

		assertType("array{'zib', 'zib 2'}", $list);
		shuffle($list);
		assertType("non-empty-list<'zib'|'zib 2'>", $list);

		assertType("list<'zib'|'zib 2'>", array_slice($list, -1));
		assertType("non-empty-list<'zib'|'zib 2'>", array_slice($list, 0));
		assertType("list<'zib'|'zib 2'>", array_slice($list, 1)); // could be non-empty-array
		assertType("list<'zib'|'zib 2'>", array_slice($list, 2));

		assertType("list<'zib'|'zib 2'>", array_slice($list, -1, 1));
		assertType("non-empty-list<'zib'|'zib 2'>", array_slice($list, 0, 1));
		assertType("list<'zib'|'zib 2'>", array_slice($list, 1, 1)); // could be non-empty-array
		assertType("list<'zib'|'zib 2'>", array_slice($list, 2, 1));

		assertType("list<'zib'|'zib 2'>", array_slice($list, -1, 2));
		assertType("non-empty-list<'zib'|'zib 2'>", array_slice($list, 0, 2));
		assertType("list<'zib'|'zib 2'>", array_slice($list, 1, 2)); // could be non-empty-array
		assertType("list<'zib'|'zib 2'>", array_slice($list, 2, 2));

		assertType("list<'zib'|'zib 2'>", array_slice($list, -1, 3));
		assertType("non-empty-list<'zib'|'zib 2'>", array_slice($list, 0, 3));
		assertType("list<'zib'|'zib 2'>", array_slice($list, 1, 3)); // could be non-empty-array
		assertType("list<'zib'|'zib 2'>", array_slice($list, 2, 3));

		assertType("array<0|1, 'zib'|'zib 2'>", array_slice($list, -1, 3, true)); // SKIP: a slice of a shuffled shape does not know how many entries it had
		assertType("non-empty-list<'zib'|'zib 2'>", array_slice($list, 0, 3, true));
		assertType("array<0|1, 'zib'|'zib 2'>", array_slice($list, 1, 3, true)); // could be non-empty-array // SKIP: a slice of a shuffled shape does not know how many entries it had
		assertType("array<0|1, 'zib'|'zib 2'>", array_slice($list, 2, 3, true)); // SKIP: a slice of a shuffled shape does not know how many entries it had

		assertType("list<'zib'|'zib 2'>", array_slice($list, -1, 3, false));
		assertType("non-empty-list<'zib'|'zib 2'>", array_slice($list, 0, 3, false));
		assertType("list<'zib'|'zib 2'>", array_slice($list, 1, 3, false)); // could be non-empty-array
		assertType("list<'zib'|'zib 2'>", array_slice($list, 2, 3, false));
	}

	/**
	 * @param array<int, string> $strings
	 * @param 0|1 $maybeZero
	 */
	public function arrayVariants(array $strings, $maybeZero): void
	{
		assertType("array<int, string>", $strings);
		assertType("list<string>", array_slice($strings, 0));
		assertType("list<string>", array_slice($strings, 1));
		assertType("list<string>", array_slice($strings, $maybeZero));

		if (count($strings) > 0) {
			assertType("non-empty-array<int, string>", $strings);
			assertType("non-empty-list<string>", array_slice($strings, 0));
			assertType("list<string>", array_slice($strings, 1));
			assertType("list<string>", array_slice($strings, $maybeZero));
		}
	}
}
