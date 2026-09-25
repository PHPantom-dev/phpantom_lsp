<?php

namespace NonEmptyArrayKeyType;

use function PHPStan\Testing\assertType;

class Foo
{

	/**
	 * @param \stdClass[] $items
	 */
	public function doFoo(array $items)
	{
		assertType('array<stdClass>', $items);

		if (count($items) > 0) {
			assertType('non-empty-array<stdClass>', $items);
			foreach ($items as $i => $val) {
				assertType('(int|string)', $i);
				assertType('stdClass', $val);
			}
		}
	}

	/**
	 * @param \stdClass[] $items
	 */
	public function doBar(array $items)
	{
		foreach ($items as $i => $val) {
			assertType('(int|string)', $i);
			assertType('stdClass', $val);
		}
	}

}
