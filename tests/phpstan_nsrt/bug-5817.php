<?php

declare(strict_types=1);

namespace Bug5817;

use ArrayAccess;
use Countable;
use DateTimeInterface;
use Iterator;
use JsonSerializable;

use function count;
use function current;
use function key;
use function next;
use function PHPStan\Testing\assertType;

/**
 * @implements ArrayAccess<int, DateTimeInterface>
 * @implements Iterator<int, DateTimeInterface>
 */
class MyContainer implements
	ArrayAccess,
	Countable,
	Iterator,
	JsonSerializable
{
	/** @var array<int, DateTimeInterface> */
	protected $items = [];

	public function add(DateTimeInterface $item, int $offset = null): self
	{
		$this->offsetSet($offset, $item);
		return $this;
	}

	public function count(): int
	{
		return count($this->items);
	}

	/** @return DateTimeInterface|false */
	#[\ReturnTypeWillChange]
	public function current()
	{
		return current($this->items);
	}

	/** @return DateTimeInterface|false */
	#[\ReturnTypeWillChange]
	public function next()
	{
		return next($this->items);
	}

	/** @return int|null */
	public function key(): ?int
	{
		return key($this->items);
	}

	public function valid(): bool
	{
		return $this->key() !== null;
	}

	/** @return DateTimeInterface|false */
	#[\ReturnTypeWillChange]
	public function rewind()
	{
		return reset($this->items);
	}

	/** @param mixed $offset */
	public function offsetExists($offset): bool
	{
		return isset($this->items[$offset]);
	}

	/** @param mixed $offset */
	public function offsetGet($offset): ?DateTimeInterface
	{
		return $this->items[$offset] ?? null;
	}

	/**
	 * @param mixed $offset
	 * @param mixed $value
	 */
	public function offsetSet($offset, $value): void
	{
		assert($value instanceof DateTimeInterface);
		if ($offset === null) { // append
			$this->items[] = $value;
		} else {
			$this->items[$offset] = $value;
		}
	}

	/** @param mixed $offset */
	public function offsetUnset($offset): void
	{
		unset($this->items[$offset]);
	}

	/** @return DateTimeInterface[] */
	public function jsonSerialize(): array
	{
		return $this->items;
	}
}

class Foo
{

	public function doFoo()
	{
		$container = (new MyContainer())->add(new \DateTimeImmutable());

		foreach ($container as $k => $item) {
			assertType('int', $k);
			assertType(DateTimeInterface::class, $item);
		}
	}

}
