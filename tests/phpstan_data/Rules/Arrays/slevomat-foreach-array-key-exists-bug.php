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
						assertType('array{itemsCount: mixed, interval: mixed}', $intervalResults[$key]); // SKIP: array shape unions are merged differently: optional keys are lost and array|array{…} is not simplified
						$intervalResults[$key]['itemsCount'] += $itemsCount;
						assertType('array{itemsCount: (array|float|int), interval: mixed}', $intervalResults[$key]); // SKIP: array shape unions are merged differently: optional keys are lost and array|array{…} is not simplified
					} else {
						assertType('array<array{itemsCount: mixed, interval: mixed}>', $intervalResults); // SKIP: array shape unions are merged differently: optional keys are lost and array|array{…} is not simplified
						assertType('array{itemsCount: mixed, interval: mixed}', $intervalResults[$key]); // SKIP: array shape unions are merged differently: optional keys are lost and array|array{…} is not simplified
						$intervalResults[$key] = [
							'itemsCount' => $itemsCount,
							'interval' => $percentageInterval,
						];
						assertType('non-empty-array<array{itemsCount: mixed, interval: mixed}>', $intervalResults); // SKIP: array shape unions are merged differently: optional keys are lost and array|array{…} is not simplified
						assertType('array{itemsCount: mixed, interval: mixed}', $intervalResults[$key]);
					}
				}
			}
		}

		assertType('array<array{itemsCount: mixed, interval: mixed}>', $intervalResults); // SKIP: array shape unions are merged differently: optional keys are lost and array|array{…} is not simplified
		foreach ($intervalResults as $data) {
			echo $data['interval'];
		}
	}

}
