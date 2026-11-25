<?php

namespace Bug5628;

use function PHPStan\Testing\assertType;

final class Cards
{
	public const SUIT_HEARTS   = 'hearts';
	public const SUIT_DIAMONDS = 'diamonds';
	public const SUIT_CLUBS    = 'clubs';
	public const SUIT_SPADES   = 'spades';

	/**
	 * @phpstan-return self::SUIT_* One of the suits chosen at random
	 */
	public function getRandomSuit(): string
	{
		// Chosen by fair dice roll. Guaranteed to be random.
		return self::SUIT_HEARTS;
	}

	public function doFoo()
	{
		assertType('\'clubs\'|\'diamonds\'|\'hearts\'|\'spades\'', $this->getRandomSuit());
	}
}
