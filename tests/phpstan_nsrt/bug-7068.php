<?php

namespace Bug7068;

use function PHPStan\Testing\assertType;

class Foo
{

	/**
	 * @template T
	 * @param array<T> ...$arrays
	 * @return array<T>
	 */
	function merge(array ...$arrays): array {
		return array_merge(...$arrays);
	}

	public function doFoo(): void
	{
		assertType('array<1|2|3|4|5>', $this->merge([1, 2], [3, 4], [5]));
		assertType('array<1|2|\'bar\'|\'foo\'>', $this->merge([1, 2], ['foo', 'bar']));
	}

}
