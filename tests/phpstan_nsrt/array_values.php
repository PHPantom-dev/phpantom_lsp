<?php // lint >= 8.0

namespace ArrayValues;

use function PHPStan\Testing\assertType;

class HelloWorld
{
	public function foo1($mixed): void
	{
		if(is_array($mixed)) {
			assertType('list<mixed>', array_values($mixed));
		} else {
		}
	}

	/**
	 * @param list<string> $list
	 */
	public function foo2($list): void
	{
		if(is_array($list)) {
			assertType('list<string>', array_values($list));
		} else {
			assertType('*NEVER*', $list);
			assertType('*NEVER*', array_values($list));
		}
	}

	public function constantArrayType(): void
	{
		$numbers = array_filter(
			[1 => 'a', 2 => 'b', 3 => 'c'],
			static fn ($value) => mt_rand(0, 1) === 0,
		);
		assertType("list{0?: 'a'|'b'|'c', 1?: 'b'|'c', 2?: 'c'}", array_values($numbers)); // SKIP: `array_values()` on a shape with optional keys gives a generic list
	}

	/**
	 * @param array<string, non-empty-array<string, int>> $a
	 */
	public function arrayMap(array $a): void
	{
		assertType('array<string, non-empty-list<int>>', array_map(array_values(...), $a)); // SKIP: a generic function passed as a first-class callable to `array_map()` leaves its template unbound
	}
}
