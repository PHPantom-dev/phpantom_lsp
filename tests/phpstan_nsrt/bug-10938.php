<?php declare(strict_types = 1);

namespace Bug10938;

use function PHPStan\Testing\assertType;

/**
 * @template TKey
 * @template TValue
 * @extends \IteratorAggregate<TKey, TValue>
 */
interface Collection extends \IteratorAggregate
{
	/**
	 * @return (TValue is never ? true : bool)
	 */
	function isEmpty(): bool;
}

/** @param Collection<never, never> $c */
function emptyCollection(Collection $c): void {
	assertType('true', $c->isEmpty());
}

/** @param Collection<int, string> $c */
function nonEmptyCollection(Collection $c): void {
	assertType('bool', $c->isEmpty());
}
