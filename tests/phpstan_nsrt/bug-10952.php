<?php declare(strict_types = 1);

namespace Bug10952;

use function PHPStan\Testing\assertType;

class HelloWorld
{
	/**
	* @return array<int, string>
	*/
	public function getArray(): array
	{
		return array_fill(0, random_int(0, 10), 'test');
	}

	public function test(): void
	{
		$array = $this->getArray();

		if (count($array) > 1) {
			assertType('non-empty-array<int, string>', $array);
		} else {
			assertType('array<int, string>', $array);
		}

		match (true) {
			default => assertType('array<int, string>', $array),
		};
	}
}
