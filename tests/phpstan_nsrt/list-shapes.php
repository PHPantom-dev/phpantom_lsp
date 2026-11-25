<?php
namespace ListShapes;

use function PHPStan\Testing\assertType;

class Foo
{
	/**
	 * @param list{} $l1
	 * @param list{'a'} $l2
	 * @param list{0: 'a'} $l3
	 * @param list{0: 'a', 1: 'b'} $l4
	 * @param list{0: 'a', 1?: 'b'} $l5
	 * @param list{'a', 'b', ...} $l6
	 */
	public function bar($l1, $l2, $l3, $l4, $l5, $l6): void
	{
		// TODO: list{}
		assertType('array{}', $l1);
		assertType("array{'a'}", $l2);
		assertType("array{'a'}", $l3);
		assertType("array{'a', 'b'}", $l4);
		assertType("array{0: 'a', 1?: 'b'}", $l5);
		assertType("list{'a', 'b', ...}", $l6); // PHPStan says `array{'a', 'b'}`; PHPantom is more precise here: it keeps the open tail
	}
}
