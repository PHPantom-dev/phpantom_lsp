<?php // lint >= 8.0

namespace Bug9456;

use ArrayAccess;

use function PHPStan\Testing\assertType;

/**
 * @template TKey of array-key
 * @template TValue of mixed
 *
 * @implements ArrayAccess<TKey, TValue>
 */
class Collection implements ArrayAccess {
	/**
	 * @param (callable(TValue, TKey): bool)|TValue|string $key
	 * @param TValue|string|null $operator
	 * @param TValue|null $value
	 * @return static<int<0, 1>, static<TKey, TValue>>
	 */
	public function partition($key, $operator = null, $value = null) {} // @phpstan-ignore-line

	/**
	 * @param TKey $key
	 * @return TValue
	 */
	public function offsetGet($key): mixed { return null; } // @phpstan-ignore-line

	/**
	 * @param TKey $key
	 * @return bool
	 */
	public function offsetExists($key): bool { return true; }

	/**
	 * @param TKey|null $key
	 * @param TValue $value
	 * @return void
	 */
	public function offsetSet($key, $value): void {}

	/**
	 * @param TKey $key
	 * @return void
	 */
	public function offsetUnset($key): void {}
}

class HelloWorld
{
	/**
	 * @param Collection<int, string> $collection
	 */
	public function sayHello(Collection $collection): void
	{
		$result = $collection->partition('key');

		assertType(
			'Bug9456\Collection<int<0, 1>, Bug9456\Collection<int, string>>',
			$result
		);
		assertType('Bug9456\Collection<int, string>', $result[0]);
		assertType('Bug9456\Collection<int, string>', $result[1]);

		[$one, $two] = $collection->partition('key');

		assertType('Bug9456\Collection<int, string>', $one);
		assertType('Bug9456\Collection<int, string>', $two);
	}
}
