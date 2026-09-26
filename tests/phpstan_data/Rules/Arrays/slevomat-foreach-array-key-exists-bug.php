<?php

namespace SlevomatForeachArrayKeyExistsBug;

use function PHPStan\Testing\assertType;

class Foo
{

	public function doFoo(array $percentageIntervals, array $changes): void
	{
		$intervalResults = [];
		foreach ($percentageIntervals as $percentageInterval) {
			foreach ($changes as $changeInPercents => $itemsCount) {
				if ($percentageInterval->isInInterval((float) $changeInPercents)) {
					$key = $percentageInterval->getFormatted();
					if (array_key_exists($key, $intervalResults)) {
						assertType('array{itemsCount: mixed, interval: mixed}', $intervalResults[$key]); // SKIP: array_key_exists() with a variable key does not narrow the offset read
						$intervalResults[$key]['itemsCount'] += $itemsCount;
						assertType('array{itemsCount: (array|float|int), interval: mixed}', $intervalResults[$key]); // SKIP: array_key_exists() with a variable key does not narrow the offset read
					} else {
						assertType('array<array{itemsCount: mixed, interval: mixed}>', $intervalResults); // SKIP: array_key_exists() with a variable key does not narrow the offset read
						assertType('array{itemsCount: mixed, interval: mixed}', $intervalResults[$key]); // SKIP: array_key_exists() with a variable key does not narrow the offset read
						$intervalResults[$key] = [
							'itemsCount' => $itemsCount,
							'interval' => $percentageInterval,
						];
						assertType('non-empty-array<array{itemsCount: mixed, interval: mixed}>', $intervalResults); // SKIP: array_key_exists() with a variable key does not narrow the offset read
						assertType('array{itemsCount: mixed, interval: mixed}', $intervalResults[$key]);
					}
				}
			}
		}

		assertType('array<array{itemsCount: mixed, interval: mixed}>', $intervalResults); // SKIP: array_key_exists() with a variable key does not narrow the offset read
		foreach ($intervalResults as $data) {
			echo $data['interval'];
		}
	}

}
