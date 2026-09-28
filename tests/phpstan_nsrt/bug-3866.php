<?php

namespace Bug3866;

use DateTimeImmutable;
use Ds\Set;
use Iterator;
use function PHPStan\Testing\assertType;

abstract class PHPStanBug
{
	public function test(): void
	{
		/** @var Set<class-string> $set */
		$set = new Set();
		foreach ($this->a() as $item) {
			$set->add(\get_class($item));
		}
		foreach ($this->b() as $item) {
			$set->add(\get_class($item));
		}
		foreach ($this->c() as $item) {
			$set->add($item);
		}
		assertType('Ds\Set<class-string>', $set);
		$set->sort();
	}

	/**
	 * @return Iterator<object>
	 */
	abstract public function a(): Iterator;

	/**
	 * @return Iterator<object>
	 */
	private function b(): Iterator
	{
		yield new DateTimeImmutable();
	}

	/**
	 * @return Iterator<class-string<object>>
	 */
	private function c(): Iterator
	{
		yield DateTimeImmutable::class;
	}
}

