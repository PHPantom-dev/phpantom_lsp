<?php

namespace Bug5140;

use function PHPStan\Testing\assertType;

/**
 * @template TKey of array-key
 * @template T
 */
interface Collection
{
	/**
	 * @return array<TKey, T>
	 */
	public function toArray(): array;

	/**
	 * @param TKey $key
	 * @param T $value
	 */
	public function set($key, $value): void;
}

class Foo
{

	/**
	 * @template TKey of array-key
	 * @template T of object
	 * @param Collection<TKey, T> $in
	 * @param Collection<TKey, T> $out
	 */
	function test(Collection $in, Collection $out): void
	{
		foreach($in->toArray() as $key => $value) {
			assertType('TKey of (int|string) (method Bug5140\Foo::test(), argument)', $key);
			assertType('T of object (method Bug5140\Foo::test(), argument)', $value);
			$out->set($key, $value);
		}
	}

}
