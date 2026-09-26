<?php

namespace PHPStan\Generics\GenericGeneralization;

use function PHPStan\Testing\assertType;

interface I {}
interface J {}

/**
 * @template T
 * @param T $arg
 * @return T
 */
function unbounded($arg)
{
	return $arg;
}

/**
 * @param class-string $classString
 * @param class-string<\stdClass> $genericClassString
 * @param array{foo: 42} $arrayShape
 * @param numeric-string $numericString
 * @param non-empty-string $nonEmptyString
 */
function testUnbounded(
	string $classString,
	string $genericClassString,
	string $string,
	array $arrayShape,
	string $numericString,
	string $nonEmptyString
): void {
	assertType('\'hello\'', unbounded('hello'));
	assertType('\'stdClass\'', unbounded('stdClass'));
	assertType('class-string', unbounded($classString));
	assertType('class-string<stdClass>', unbounded($genericClassString));

	assertType("'hello'|class-string", unbounded(rand(0,1) === 1 ? 'hello' : $classString));

	assertType('array{foo: 42}', unbounded($arrayShape));

	assertType('numeric-string', unbounded($numericString));
	assertType('non-empty-string', unbounded($nonEmptyString));
}

/**
 * @template T of string
 * @param T $arg
 * @return T
 */
function boundToString($arg)
{
	return $arg;
}

/**
 * @param class-string $classString
 * @param class-string<\stdClass> $genericClassString
 * @param non-empty-string $nonEmptyString
 */
function testBoundToString(
	string $classString,
	string $genericClassString,
	string $nonEmptyString,
	string $string
): void {
	assertType('\'hello\'', boundToString('hello'));
	assertType('\'stdClass\'', boundToString('stdClass'));
	assertType('class-string', boundToString($classString));
	assertType('class-string<stdClass>', boundToString($genericClassString));

	assertType('\'hello\'|class-string', boundToString(rand(0,1) === 1 ? 'hello' : $classString));
}
