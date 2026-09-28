<?php // lint >= 8.0

namespace EnumExists;

use function PHPStan\Testing\assertType;

function getEnumValue(string $enumFqcn, string $name): mixed {
	if (enum_exists($enumFqcn)) {
		assertType('class-string<UnitEnum>', $enumFqcn);
		return (new \ReflectionEnum($enumFqcn))->getCase($name)->getValue();
	}
	assertType('string', $enumFqcn);

	return null;
}

/**
 * @param class-string $enumFqcn
 */
function getEnumValueFromClassString(string $enumFqcn, string $name): mixed {
	if (enum_exists($enumFqcn)) {
		assertType('class-string<UnitEnum>', $enumFqcn);
		return (new \ReflectionEnum($enumFqcn))->getCase($name)->getValue();
	}
	assertType('class-string', $enumFqcn);

	return null;
}
