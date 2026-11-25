<?php declare(strict_types = 1);

namespace Bug14348;

use function PHPStan\Testing\assertType;

interface PositionEntityInterface {
	public function getPosition(): int;
}
interface TgEntityInterface {}

abstract class HelloWorld
{
	/**
	 * @phpstan-template T of PositionEntityInterface&TgEntityInterface
	 *
	 * @param non-empty-array<T> $tgs
	 */
	public function computeForFrontByPosition(array $tgs): void
	{
		assertType('T of Bug14348\PositionEntityInterface&Bug14348\TgEntityInterface (method Bug14348\HelloWorld::computeForFrontByPosition(), argument)', $tgs[0]);
	}
}
