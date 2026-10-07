<?php

namespace Bug987;

use function PHPStan\Testing\assertType;

class HelloWorld
{
	public function sayHello(?int $a, ?int $b): void
	{

		if ($a === null && $b === null) {
			throw new \LogicException();
		}

		assertType('int', $a !== null ? $a : $b);
	}

	/**
	 * @return string[]|null
	 */
	public function arrOrNull(): ?array
	{
		return [];
	}

	/**
	 * @return string[]
	 */
	public function otherGetArray(): array
	{
		return [];
	}

	/**
	 * @return string[]
	 */
	function hello(self $class, bool $boolOption): array
	{
		$myArray = $class->arrOrNull();
		if ($boolOption && $myArray === null) {
			throw new \Exception('');
		}
		if (!$boolOption) {
			$myArray = $class->otherGetArray();
		}

		assertType('array<string>', $myArray);

		return $myArray;
	}

}
