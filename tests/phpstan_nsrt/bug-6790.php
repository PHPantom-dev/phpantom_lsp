<?php // lint >= 8.0

declare(strict_types = 1);

namespace Bug6790;

use function PHPStan\Testing\assertType;

/**
 * @template T
 */
class Repository
{
	/**
	 * @param array<T> $items
	 */
	public function __construct(private array $items) {}

	/**
	 * @return ?T
	 */
	public function find(string $id)
	{
		return $this->items[$id] ?? null;
	}
}

/**
 * @template T
 */
class Repository2
{
	/**
	 * @param array<T> $items
	 */
	public function __construct(private array $items) {}

	/**
	 * @return T|null
	 */
	public function find(string $id)
	{
		return $this->items[$id] ?? null;
	}
}

class Foo
{

	/**
	 * @param Repository<string> $r
	 * @return void
	 */
	public function doFoo(Repository $r): void
	{
		assertType('string|null', $r->find('foo'));
	}

	/**
	 * @param Repository2<string> $r
	 * @return void
	 */
	public function doFoo2(Repository2 $r): void
	{
		assertType('string|null', $r->find('foo'));
	}

}
