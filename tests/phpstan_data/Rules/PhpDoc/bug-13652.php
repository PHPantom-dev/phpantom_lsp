<?php // lint >= 8.0

namespace Bug13652;

use function PHPStan\Testing\assertType;

/**
 * @phpstan-type Bob array{a: string, b: bool, c: int, d: float}
 */
class Y {
	public function __construct(/** @var Bob */ public array $bob) {}

	/**
	 * @template TKey of key-of<Bob>
	 * @param TKey $key
	 * @return Bob[TKey]
	 */
	public function x(string $key): string|bool|int|float
	{
		return $this->bob[$key];
	}

	/**
	 * @template TKey of key-of<Bob>
	 * @param TKey $key
	 * @return Bob[TKey]
	 */
	public function x2(string $key): string|bool|int
	{
		return $this->bob[$key];
	}

	/**
	 * @template TKey of key-of<Bob>
	 * @param TKey $key
	 * @param Bob[TKey] $value
	 */
	public function x3(string $key, string|bool|int|float $value): void
	{

	}

	/**
	 * @template TKey of key-of<Bob>
	 * @param TKey $key
	 * @param Bob[TKey] $value
	 */
	public function x4(string $key, string|bool|int $value): void
	{

	}
}

function (): void {
	$y = new Y(['a' => 'y', 'b' => true, 'c' => 99, 'd' => 4.2]);
	assertType('bool', $y->x('b'));
};
