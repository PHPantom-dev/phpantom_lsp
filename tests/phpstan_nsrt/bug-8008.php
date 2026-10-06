<?php

namespace Bug8008;

use function \PHPStan\Testing\assertType;

/**
 * @template TValue
 */
class Collection
{
	/**
	 * @param array<TValue> $items
	 */
	public function __construct(
		public array $items,
	) {
	}

	/**
	 * @return array<TValue>
	 */
	public function all() {
		return $this->items;
	}
}

/**
 * @template TValue of object
 *
 * @mixin Collection<TValue>
 */
class Paginator
{
	/**
	 * @var Collection<TValue>
	 */
	public Collection $collection;

	/**
	 * @param array<TValue> $items
	 */
	public function __construct(public array $items)
	{
		$this->collection = new Collection($items);
	}
}

class MyObject {}


function (): void {
	$paginator = new Paginator([new MyObject()]);

	assertType('Bug8008\Paginator<Bug8008\MyObject>', $paginator);
	assertType('array<Bug8008\MyObject>', $paginator->items);
	assertType('Bug8008\Collection<Bug8008\MyObject>', $paginator->collection);
	assertType('array<Bug8008\MyObject>', $paginator->collection->items);

	assertType('array<Bug8008\MyObject>', $paginator->all());
};
