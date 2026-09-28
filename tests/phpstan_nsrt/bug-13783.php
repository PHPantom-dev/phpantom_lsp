<?php

namespace Bug13783;

use ReflectionClass;
use function PHPStan\Testing\assertType;

class A {}
class B {}

/**
 * @param class-string $c
 */
function sayHello(string $c): void
{
	$reflect = new ReflectionClass($c);
	if ($reflect->isSubClassOf(A::class) || $reflect->isSubClassOf(B::class)) {
		assertType('ReflectionClass<Bug13783\A>|ReflectionClass<Bug13783\B>', $reflect);
	}
}

/**
 * @param class-string $c
 */
function sayHello2(string $c): void
{
	$reflect = new ReflectionClass($c);
	if ($reflect->isSubClassOf(A::class)) {
		assertType('ReflectionClass<Bug13783\A>', $reflect);
	} elseif ($reflect->isSubClassOf(B::class)) {
		assertType('ReflectionClass<Bug13783\B>', $reflect);
	}
}
