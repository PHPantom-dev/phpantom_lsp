<?php

namespace Bug5017;

use function PHPStan\Testing\assertType;

class Foo
{

	public function doFoo()
	{
		$items = [0, 1, 2, 3, 4];

		while ($items) {
			assertType('non-empty-list<0|1|2|3|4>', $items); // PHPStan says `int<min, 4>`; PHPantom is more precise here
			$batch = array_splice($items, 0, 2);
			assertType('list<0|1|2|3|4>', $items); // PHPStan says `int<min, 4>`; PHPantom is more precise here
			assertType('non-empty-list<0|1|2|3|4>', $batch); // PHPStan says `int<min, 4>`; PHPantom is more precise here
		}
	}

	/**
	 * @param int[] $items
	 */
	public function doBar($items)
	{
		while ($items) {
			assertType('non-empty-array<int>', $items);
			$batch = array_splice($items, 0, 2);
			assertType('array<(int<0, max>|string), int>', $items);
			assertType('non-empty-array<int>', $batch); // PHPStan says `array<int>`, but a slice from the start of a non-empty array keeps its first entry
		}
	}

	public function doBar2()
	{
		$items = [0, 1, 2, 3, 4];
		assertType('array{0, 1, 2, 3, 4}', $items);
		$batch = array_splice($items, 0, 2);
		assertType('array{2, 3, 4}', $items);
		assertType('array{0, 1}', $batch);
	}

	/**
	 * @param int[] $ints
	 * @param string[] $strings
	 */
	public function doBar3(array $ints, array $strings)
	{
		$removed = array_splice($ints, 0, 2, $strings);
		assertType('array<int>', $removed);
		assertType('array<(int<0, max>|string), int|string>', $ints);
		assertType('array<string>', $strings);
	}

}
