<?php declare(strict_types=1);

namespace Bug8225;

use function PHPStan\Testing\assertType;

class HelloWorld
{
	/**
	 * @param array{
	 *     notImportant: bool,
	 *     attributesRequiredLogistic?: array<int>,
	 * } $array
	 */
	public function sayHello(array $array, string $string): void
	{
		assertType('array{notImportant: bool, attributesRequiredLogistic?: array<int>}', $array);
		unset($array[$string]);
		assertType('array{notImportant?: bool, attributesRequiredLogistic?: array<int>}', $array);
	}

	public function edgeCase(): void
	{
		$arr = [1,2,3];
		unset($arr['1']);
		assertType('array{0: 1, 2: 3}', $arr);
	}
}
