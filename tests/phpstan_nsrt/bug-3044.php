<?php

namespace Bug3044;

use function PHPStan\Testing\assertType;

class HelloWorld
{
	/**
	 * @return array<int, array<int, float>>
	 */
	private function getIntArrayFloatArray(): array
	{
		return [
			0 => [1.1, 2.2, 3.3],
			1 => [1.1, 2.2, 3.3],
			2 => [1.1, 2.2, 3.3],
		];
	}

	/**
	 * @return array<int, array<int, float>>
	 */
	public function invalidType(): void
	{
		$X = $this->getIntArrayFloatArray();
		$L = $this->getIntArrayFloatArray();

		$n = 3;
		$m = 3;

		for ($k = 0; $k < $m; ++$k) {
			for ($j = 0; $j < $n; ++$j) {
				for ($i = 0; $i < $k; ++$i) {
					$X[$k][$j] -= $X[$i][$j] * $L[$k][$i];
				}
			}
		}

		assertType('array<int, array<int, float>>', $X);
	}
}
