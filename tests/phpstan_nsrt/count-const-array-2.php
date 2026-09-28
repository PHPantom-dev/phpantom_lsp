<?php declare(strict_types = 1);

namespace CountConstArray2;

use function PHPStan\Testing\assertType;

class HelloWorld
{
	/**
	 * @param int<1, max> $limit
	 * @return list<\stdClass>
	 */
	public function searchRecommendedMinPrices(int $limit): array
	{
		$bestMinPrice = new \stdClass();
		$limit--;
		if ($limit === 0) {
			return [$bestMinPrice];
		}

		$otherMinPrices = [new \stdClass()];
		while (count($otherMinPrices) < $limit) {
			$otherMinPrice = new \stdClass();
			if (rand(0, 1)) {
				$otherMinPrice = null;
			}
			if ($otherMinPrice === null) {
				break;
			}
			array_unshift($otherMinPrices, $otherMinPrice);
		}
		assertType('non-empty-list<stdClass>', $otherMinPrices);
		return [$bestMinPrice, ...$otherMinPrices];
	}
}
