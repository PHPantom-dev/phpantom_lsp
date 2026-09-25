<?php // lint >= 8.0

namespace Bug7944;

use function PHPStan\Testing\assertType;

/**
 * @template TValue
 */
final class Value
{
	/** @var TValue */
	public readonly mixed $value;

	/**
	 * @param TValue $value
	 */
	public function __construct(mixed $value)
	{
		$this->value           = $value;
	}
}

/**
 * @param non-empty-string $p
 */
function test($p): void {
	$value = new Value($p);
	assertType('Bug7944\\Value<non-empty-string>', $value);
};

