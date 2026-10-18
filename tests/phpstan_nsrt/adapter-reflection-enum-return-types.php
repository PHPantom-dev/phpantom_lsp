<?php

namespace AdapterReflectionEnumReturnTypes;

use PHPStan\BetterReflection\Reflection\Adapter\ReflectionEnum;
use PHPStan\BetterReflection\Reflection\Adapter\ReflectionEnumBackedCase;
use PHPStan\BetterReflection\Reflection\Adapter\ReflectionEnumUnitCase;
use PHPStan\BetterReflection\Reflection\Adapter\ReflectionType;
use function PHPStan\Testing\assertType;

function (ReflectionEnum $r, string $s): void {
	assertType('string|false', $r->getDocComment()); // SKIP: the imported BetterReflection adapters are not available
	assertType('PHPStan\BetterReflection\Reflection\Adapter\ReflectionClassConstant|false', $r->getReflectionConstant($s)); // SKIP: the imported BetterReflection adapters are not available
	assertType('PHPStan\BetterReflection\Reflection\Adapter\ReflectionClass|false', $r->getParentClass()); // SKIP: the imported BetterReflection adapters are not available
	assertType('PHPStan\BetterReflection\Reflection\Adapter\ReflectionNamedType|null', $r->getBackingType()); // SKIP: the imported BetterReflection adapters are not available
};

function (ReflectionEnumBackedCase $r): void {
	assertType('string|false', $r->getDocComment()); // SKIP: the imported BetterReflection adapters are not available
	assertType(ReflectionType::class . '|null', $r->getType()); // SKIP: the imported BetterReflection adapters are not available
};

function (ReflectionEnumUnitCase $r): void {
	assertType('string|false', $r->getDocComment()); // SKIP: the imported BetterReflection adapters are not available
	assertType(ReflectionType::class . '|null', $r->getType()); // SKIP: the imported BetterReflection adapters are not available
};
