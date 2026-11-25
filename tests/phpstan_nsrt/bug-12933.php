<?php // lint >= 8.0

namespace Bug12933;

use function PHPStan\Testing\assertType;

/**
 * @phpstan-type record array{id: positive-int, name: string}
 */
class Collection
{
	/** @param list<record> $list */
	public function __construct(
		public array $list
	)
	{
	}

	public function updateNameIsset(int $index, string $name): void
	{
		assert(isset($this->list[$index]));
	}

	public function updateNameArrayKeyExists(int $index, string $name): void
	{
		assert(array_key_exists($index, $this->list));
		assertType('int<0, max>', $index);
	}

	/**
	 * @param int<-5, 5> $index
	 */
	public function issetNarrowsIntRange(int $index, string $name): void
	{
		assert(isset($this->list[$index]));
	}

	/**
	 * @param int<5, 15> $index
	 */
	public function issetNotWidensIntRange(int $index, string $name): void
	{
		assert(isset($this->list[$index]));
		assertType('int<5, 15>', $index);
	}
}
