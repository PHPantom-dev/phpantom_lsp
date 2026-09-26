<?php

namespace Bug4415Rule;

use function PHPStan\Testing\assertType;

/**
 * @template T
 * @extends \IteratorAggregate<T>
 */
interface CollectionInterface extends \IteratorAggregate
{
	/**
	 * @param T $item
	 */
	public function has($item): bool;

	/**
	 * @return self<T>
	 */
	public function sort(): self;
}

/**
 * @template T
 * @extends CollectionInterface<T>
 */
interface MutableCollectionInterface extends CollectionInterface
{
	/**
	 * @param T $item
	 * @phpstan-return self<T>
	 */
	public function add($item): self;
}

/**
 * @extends CollectionInterface<Category>
 */
interface CategoryCollectionInterface extends CollectionInterface
{
	public function has($item): bool;

	/**
	 * @phpstan-return \Iterator<Category>
	 */
	public function getIterator(): \Iterator;
}

/**
 * @extends MutableCollectionInterface<Category>
 */
interface MutableCategoryCollectionInterface extends CategoryCollectionInterface, MutableCollectionInterface
{
}

class CategoryCollection implements MutableCategoryCollectionInterface
{
	/** @var array<Category> */
	private $categories = [];

	public function add($item): MutableCollectionInterface
	{
		$this->categories[$item->getName()] = $item;
		return $this;
	}

	public function has($item): bool
	{
		return isset($this->categories[$item->getName()]);
	}

	public function sort(): CollectionInterface
	{
		return $this;
	}

	public function getIterator(): \Iterator
	{
		return new \ArrayIterator($this->categories);
	}
}

class Category {
	public function getName(): string
	{
		return '';
	}
}

function (CategoryCollection $c): void {
	foreach ($c as $k => $v) {
		assertType('mixed', $k);
		assertType(Category::class, $v);
	}
};
