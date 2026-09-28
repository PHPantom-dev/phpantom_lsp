<?php

namespace Bug5508;

use function PHPStan\Testing\assertType;

/**
 * @template TKey as array-key
 * @template TValue
 */
class Collection
{
	/**
	 * @var array<TKey, TValue>
	 */
	protected $items = [];

	/**
	 * @param  array<TKey, TValue>  $items
	 * @return void
	 */
	public function __construct($items)
	{
		$this->items = $items;
	}

	/**
	 * @template TMapValue
	 *
	 * @param  callable(TValue, TKey): TMapValue  $callback
	 * @return self<TKey, TMapValue>
	 */
	public function map(callable $callback)
	{
		$keys = array_keys($this->items);

		$items = array_map($callback, $this->items, $keys);

		return new self(array_combine($keys, $items));
	}

	/**
	 * @return array<TKey, TValue>
	 */
	public function all()
	{
		return $this->items;
	}
}

function (): void {
	$result = (new Collection(['book', 'cars']))->map(function($category) {
		return $category;
	})->all();

	assertType('array<int, string>', $result);
};
