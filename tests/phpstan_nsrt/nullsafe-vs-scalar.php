<?php

namespace NullsafeVsScalar;

use function PHPStan\Testing\assertType;

class HelloWorld
{
	public function aaa(?\DateTimeImmutable $date): void
	{
		echo $date?->getTimestamp() > 0 ? $date->format('j') : '';
		echo $date?->getTimestamp() > 0 ? $date->format('j') : '';
		assertType('DateTimeImmutable|null', $date);
	}

	public function bbb(?\DateTimeImmutable $date): void
	{
		echo 0 < $date?->getTimestamp() ? $date->format('j') : '';
		echo 0 < $date?->getTimestamp() ? $date->format('j') : '';
		assertType('DateTimeImmutable|null', $date);
	}

	/** @param mixed $date */
	public function ccc($date): void
	{
		if ($date?->getTimestamp() > 0) {
		}

		echo $date?->getTimestamp() > 0 ? $date->format('j') : '';
		echo $date?->getTimestamp() > 0 ? $date->format('j') : '';
		assertType('mixed', $date);
	}
}
