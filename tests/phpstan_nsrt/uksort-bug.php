<?php

namespace UkSortBug;

use function PHPStan\Testing\assertType;

class Foo
{

}

class Bar
{

	/** @var Foo[] */
	private $unknownKeys;

	/** @var array<string, Foo> */
	private $stringKeys;

	/** @var array<int, Foo> */
	private $intKeys;

	/**
	 * @template T of array-key
	 *
	 * @param array<T, mixed> $one
	 * @param callable(T, T): int $two
	 * @return T
	 */
	function uksort(
		array &$one,
		callable $two
	) {

	}

	public function doFoo(): void
	{
		assertType('(int|string)', $this->uksort($this->unknownKeys, function (string $a, string $b): int {
			return 1;
		}));

		assertType('string', $this->uksort($this->stringKeys, function (string $a, string $b): int {
			return 1;
		}));
		assertType('string', $this->uksort($this->stringKeys, function (int $a, int $b): int {
			return 1;
		}));
		assertType('string', $this->uksort($this->stringKeys, function (int $a, string $b): int {
			return 1;
		}));

		assertType('int', $this->uksort($this->intKeys, function (int $a, int $b): int {
			return 1;
		}));
		assertType('int', $this->uksort($this->intKeys, function (string $a, string $b): int {
			return 1;
		}));
		assertType('int', $this->uksort($this->intKeys, function (int $a, string $b): int {
			return 1;
		}));
	}

}
