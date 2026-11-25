<?php // lint >= 8.1

namespace Bug9881;

use BackedEnum;
use function PHPStan\Testing\assertType;

class HelloWorld
{
	/**
	 * @template B of BackedEnum
	 * @param B[] $enums
	 * @return value-of<B>[]
	 */
	public static function arrayEnumToStrings(array $enums): array
	{
		return array_map(static fn (BackedEnum $code): string|int => $code->value, $enums);
	}
}

enum Test: string {
	case DA = 'da';
}

function (Test ...$da): void {
	assertType('array<\'da\'>', HelloWorld::arrayEnumToStrings($da));
};
