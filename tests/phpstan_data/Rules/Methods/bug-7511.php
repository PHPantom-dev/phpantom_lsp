<?php

namespace Bug7511;

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
	 * @param iterable<T> $tgs
	 *
	 * @return array<T>
	 *
	 * @throws \Exception
	 */
	public function computeForFrontByPosition($tgs)
	{
		/** @phpstan-var array<T> $res */
		$res = [];

		assertType('T of Bug7511\PositionEntityInterface&Bug7511\TgEntityInterface (method Bug7511\HelloWorld::computeForFrontByPosition(), parameter)', $res[1]);

		foreach ($tgs as $tgItem) {
			$position = $tgItem->getPosition();

			if (!isset($res[$position])) {
				assertType('T of Bug7511\PositionEntityInterface&Bug7511\TgEntityInterface (method Bug7511\HelloWorld::computeForFrontByPosition(), argument)', $tgItem);
				$res[$position] = $tgItem;
			} else {
				assertType('T of Bug7511\PositionEntityInterface&Bug7511\TgEntityInterface (method Bug7511\HelloWorld::computeForFrontByPosition(), argument)', $tgItem);
				assertType('T of Bug7511\PositionEntityInterface&Bug7511\TgEntityInterface (method Bug7511\HelloWorld::computeForFrontByPosition(), parameter)', $res[$position]);
				$tgItemToKeep   = $this->compare($tgItem, $res[$position]);
				assertType('T of Bug7511\PositionEntityInterface&Bug7511\TgEntityInterface (method Bug7511\HelloWorld::computeForFrontByPosition(), parameter)', $tgItemToKeep);
				$res[$position] = $tgItemToKeep;
			}
		}
		ksort($res);

		assertType('array<T of Bug7511\PositionEntityInterface&Bug7511\TgEntityInterface (method Bug7511\HelloWorld::computeForFrontByPosition(), parameter)>', $res);

		return $res;
	}

	/**
	 * @phpstan-template T of PositionEntityInterface&TgEntityInterface
	 *
	 * @param iterable<T> $tgs
	 *
	 * @return array<T>
	 *
	 * @throws \Exception
	 */
	public function computeForFrontByPosition2($tgs)
	{
		/** @phpstan-var array<T> $res */
		$res = [];

		assertType('array<T of Bug7511\PositionEntityInterface&Bug7511\TgEntityInterface (method Bug7511\HelloWorld::computeForFrontByPosition2(), parameter)>', $res);

		foreach ($tgs as $tgItem) {
			$position = $tgItem->getPosition();

			assertType('T of Bug7511\PositionEntityInterface&Bug7511\TgEntityInterface (method Bug7511\HelloWorld::computeForFrontByPosition2(), parameter)', $res[$position]);
			if (isset($res[$position])) {
				assertType('T of Bug7511\PositionEntityInterface&Bug7511\TgEntityInterface (method Bug7511\HelloWorld::computeForFrontByPosition2(), parameter)', $res[$position]);
			}
		}
		assertType('array<T of Bug7511\PositionEntityInterface&Bug7511\TgEntityInterface (method Bug7511\HelloWorld::computeForFrontByPosition2(), parameter)>', $res);

		return $res;
	}

	/**
	 * @phpstan-template S of TgEntityInterface
	 * @phpstan-param S $nextTg
	 * @phpstan-param S $currentTg
	 * @phpstan-return S
	 */
	abstract protected function compare(TgEntityInterface $nextTg, TgEntityInterface $currentTg): TgEntityInterface;
}
