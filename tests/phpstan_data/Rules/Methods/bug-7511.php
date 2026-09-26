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

		assertType('T of Bug7511\PositionEntityInterface&Bug7511\TgEntityInterface (method Bug7511\HelloWorld::computeForFrontByPosition(), parameter)', $res[1]); // SKIP: an inline @phpstan-var above an assignment is ignored (plain @var works)

		foreach ($tgs as $tgItem) {
			$position = $tgItem->getPosition();

			if (!isset($res[$position])) {
				assertType('T of Bug7511\PositionEntityInterface&Bug7511\TgEntityInterface (method Bug7511\HelloWorld::computeForFrontByPosition(), argument)', $tgItem); // SKIP: a method template with a bound is shown as its bound inside the method
				$res[$position] = $tgItem;
			} else {
				assertType('T of Bug7511\PositionEntityInterface&Bug7511\TgEntityInterface (method Bug7511\HelloWorld::computeForFrontByPosition(), argument)', $tgItem); // SKIP: a method template with a bound is shown as its bound inside the method
				assertType('T of Bug7511\PositionEntityInterface&Bug7511\TgEntityInterface (method Bug7511\HelloWorld::computeForFrontByPosition(), parameter)', $res[$position]); // SKIP: a method template with a bound is shown as its bound inside the method
				$tgItemToKeep   = $this->compare($tgItem, $res[$position]);
				assertType('T of Bug7511\PositionEntityInterface&Bug7511\TgEntityInterface (method Bug7511\HelloWorld::computeForFrontByPosition(), parameter)', $tgItemToKeep); // SKIP: a method template with a bound is shown as its bound inside the method
				$res[$position] = $tgItemToKeep;
			}
		}
		ksort($res);

		assertType('array<T of Bug7511\PositionEntityInterface&Bug7511\TgEntityInterface (method Bug7511\HelloWorld::computeForFrontByPosition(), parameter)>', $res); // SKIP: a method template with a bound is shown as its bound inside the method

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

		assertType('array<T of Bug7511\PositionEntityInterface&Bug7511\TgEntityInterface (method Bug7511\HelloWorld::computeForFrontByPosition2(), parameter)>', $res); // SKIP: an inline @phpstan-var above an assignment is ignored (plain @var works)

		foreach ($tgs as $tgItem) {
			$position = $tgItem->getPosition();

			assertType('T of Bug7511\PositionEntityInterface&Bug7511\TgEntityInterface (method Bug7511\HelloWorld::computeForFrontByPosition2(), parameter)', $res[$position]); // SKIP: an inline @phpstan-var above an assignment is ignored (plain @var works)
			if (isset($res[$position])) {
				assertType('T of Bug7511\PositionEntityInterface&Bug7511\TgEntityInterface (method Bug7511\HelloWorld::computeForFrontByPosition2(), parameter)', $res[$position]); // SKIP: an inline @phpstan-var above an assignment is ignored (plain @var works)
			}
		}
		assertType('array<T of Bug7511\PositionEntityInterface&Bug7511\TgEntityInterface (method Bug7511\HelloWorld::computeForFrontByPosition2(), parameter)>', $res); // SKIP: an inline @phpstan-var above an assignment is ignored (plain @var works)

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
