<?php

namespace AdapterReflectionEnumReturnTypes;

use PHPStan\BetterReflection\Reflection\Adapter\ReflectionEnum;
use PHPStan\BetterReflection\Reflection\Adapter\ReflectionEnumBackedCase;
use PHPStan\BetterReflection\Reflection\Adapter\ReflectionEnumUnitCase;
use PHPStan\BetterReflection\Reflection\Adapter\ReflectionType;
use function PHPStan\Testing\assertType;

function (ReflectionEnum $r, string $s): void {
	assertType('string|false', $r->getDocComment());
	assertType('PHPStan\BetterReflection\Reflection\Adapter\ReflectionClassConstant|false', $r->getReflectionConstant($s));
	assertType('PHPStan\BetterReflection\Reflection\Adapter\ReflectionClass|false', $r->getParentClass());
	assertType('PHPStan\BetterReflection\Reflection\Adapter\ReflectionNamedType|null', $r->getBackingType());
};

function (ReflectionEnumBackedCase $r): void {
	assertType('string|false', $r->getDocComment());
	assertType(ReflectionType::class . '|null', $r->getType());
};

function (ReflectionEnumUnitCase $r): void {
	assertType('string|false', $r->getDocComment());
	assertType(ReflectionType::class . '|null', $r->getType());
};
