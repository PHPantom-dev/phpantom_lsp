<?php // lint >= 8.1

namespace Bug9084;

use function PHPStan\Testing\assertType;

enum UnitType
{
	case Mass;

	case Length;
}

/**
 * @template TUnitType of UnitType::*
 */
interface UnitInterface
{
	public function getValue(): float;
}

/**
 * @implements UnitInterface<UnitType::Mass>
 */
enum MassUnit: int implements UnitInterface
{
	case KiloGram = 1000000;

	case Gram = 1000;

	case MilliGram = 1;

	public function getValue(): float
	{
		return $this->value;
	}
}

/**
 * @template TUnit of UnitType::*
 */
class Value
{
	public function __construct(
		public readonly float $value,
		/** @var UnitInterface<TUnit> */
		public readonly UnitInterface $unit
	) {
	}

	/**
	 * @param UnitInterface<TUnit> $unit
	 * @return Value<TUnit>
	 */
	public function convert(UnitInterface $unit): Value
	{
		return new Value($this->value / $unit->getValue(), $unit);
	}
}

/**
 * @template S
 * @param  S $value
 * @return S
 */
function duplicate($value)
{
	return clone $value;
}

function (): void {
	$a = new Value(10, MassUnit::KiloGram);
	assertType('Bug9084\Value<Bug9084\UnitType::Mass>', $a);
	$b = duplicate($a);
	assertType('Bug9084\Value<Bug9084\UnitType::Mass>', $b);
};
