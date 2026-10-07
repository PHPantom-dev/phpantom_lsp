<?php // lint >= 8.0

declare(strict_types=1);

namespace Bug9867;

use function PHPStan\Testing\assertType;

/** @extends \SplMinHeap<\DateTime> */
class MyMinHeap extends \SplMinHeap
{
	public function test(): void
	{
		assertType('DateTime', parent::current());
		assertType('DateTime', parent::extract());
		assertType('DateTime', parent::top());
	}

	public function insert(mixed $value): bool
	{
		assertType('DateTime', $value);

		return parent::insert($value);
	}

	protected function compare(mixed $value1, mixed $value2)
	{
		assertType('DateTime', $value1);
		assertType('DateTime', $value2);

		return parent::compare($value1, $value2);
	}
}

/** @extends \SplMaxHeap<\DateTime> */
class MyMaxHeap extends \SplMaxHeap
{
	public function test(): void
	{
		assertType('DateTime', parent::current());
		assertType('DateTime', parent::extract());
		assertType('DateTime', parent::top());
	}

	public function insert(mixed $value): bool
	{
		assertType('DateTime', $value);

		return parent::insert($value);
	}

	protected function compare(mixed $value1, mixed $value2)
	{
		assertType('DateTime', $value1);
		assertType('DateTime', $value2);

		return parent::compare($value1, $value2);
	}
}

/** @extends \SplHeap<\DateTime> */
abstract class MyHeap extends \SplHeap
{
	public function test(): void
	{
		assertType('DateTime', parent::current());
		assertType('DateTime', parent::extract());
		assertType('DateTime', parent::top());
	}

	public function insert(mixed $value): bool
	{
		assertType('DateTime', $value);

		return parent::insert($value);
	}
}
