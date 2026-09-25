<?php declare(strict_types = 1);

namespace Bug7996;

use function PHPStan\Testing\assertType;

class HelloWorld
{
	/**
	 * @param non-empty-array<\stdclass> $inputArray
	 * @return non-empty-array<\stdclass>
	 */
	public function filter(array $inputArray): array
	{
		$currentItem = reset($inputArray);
		$outputArray = [$currentItem]; // $outputArray is now non-empty-array
		assertType('array{stdclass}', $outputArray);

		while ($nextItem = next($inputArray)) {
			if (rand(1, 2) === 1) {
				assertType('non-empty-list<stdclass>', $outputArray);
				// The fact that this is into an if, reverts type of $outputArray to array
				$outputArray[] = $nextItem;
			}
			assertType('non-empty-list<stdclass>', $outputArray);
		}

		assertType('non-empty-list<stdclass>', $outputArray);
		return $outputArray;
	}
}
