<?php declare(strict_types = 1);

namespace Bug6462;

use function PHPStan\Testing\assertType;

class Base
{
	/**
	 * @return $this
	 */
	public function getThis(): self
	{
		return $this;
	}
}

class Child extends Base
{
	/**
	 * {@inheritDoc}
	 */
	public function getThis(): self
	{
		return $this;
	}
}

class FixedChild extends Base
{
	/**
	 * @return $this
	 */
	public function getThis(): self
	{
		return $this;
	}
}

function (Base $base, Child $child, FixedChild $fixedChild): void {
	assertType('Bug6462\Base', $base->getThis());
	assertType('Bug6462\Child', $child->getThis());

	if ($base instanceof \Traversable) {
		assertType('Bug6462\Base&Traversable', $base->getThis());
	}

	if ($child instanceof \Traversable) {
		assertType('Bug6462\Child&Traversable', $child->getThis());
	}

	if ($fixedChild instanceof \Traversable) {
		assertType('Bug6462\FixedChild&Traversable', $fixedChild->getThis());
	}
};
