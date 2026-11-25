<?php

namespace MinMax;

use function PHPStan\Testing\assertType;

function dummy4(\DateTimeInterface $dateA, ?\DateTimeInterface $dateB): void
{
	assertType('array{0: DateTimeInterface, 1?: DateTimeInterface}', array_filter([$dateA, $dateB]));
	assertType('DateTimeInterface', min(array_filter([$dateA, $dateB])));
	assertType('DateTimeInterface', max(array_filter([$dateA, $dateB])));
	assertType('array{0?: DateTimeInterface}', array_filter([$dateB]));
}

function dummy5(int $i, int $j): void
{
	assertType('array{1: true}', array_filter([false, true]));
}

function dummy6(string $s, string $t): void {
	assertType('array{0?: non-falsy-string, 1?: non-falsy-string}', array_filter([$s, $t]));
}

class HelloWorld
{
	public function setRange(int $range): void
	{
		if ($range < 0) {
			return;
		}
		assertType('int<0, 100>', min($range, 100));
		assertType('int<0, 100>', min(100, $range));
	}

	public function setRange2(int $range): void
	{
		if ($range > 100) {
			return;
		}
		assertType('int<0, 100>', max($range, 0));
		assertType('int<0, 100>', max(0, $range));
	}

	public function boundRange(): void
	{
		/**
		 * @var int<1, 6> $range
		 */
		$range = getFoo();

		assertType('int<1, 4>', min($range, 4));
		assertType('int<4, 6>', max(4, $range));
	}

	public function unionType(): void
	{
		/**
		 * @var array{0, 1, 2}|array{4, 5, 6} $numbers2
		 */
		$numbers2 = getFoo();

		assertType('0|4', min($numbers2));
		assertType('2|6', max($numbers2));
	}
}
