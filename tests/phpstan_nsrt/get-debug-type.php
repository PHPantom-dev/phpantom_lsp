<?php

namespace GetDebugType;

use function PHPStan\Testing\assertType;

final class A {}
interface B {}
interface C {}
class D {}

/**
 * @param double $d
 * @param resource $r
 * @param int|string $intOrString
 * @param array|A $arrayOrObject
 */
function doFoo(bool $b, int $i, float $f, $d, $r, string $s, array $a, $intOrString, $arrayOrObject, \stdClass $std) {
	$null = null;
	$resource = fopen('php://memory', 'r');
	$o = new \stdClass();
	$A = new A();
	$anonymous = new class {};
	$anonymousImplements = new class implements B, C {};
	$anonymousExtends = new class extends D {};
	$anonymousExtendsImplements = new class extends D implements B, C {};

	assertType("'bool'", get_debug_type($b));
	assertType("'bool'", get_debug_type(true));
	assertType("'bool'", get_debug_type(false));
	assertType("'int'", get_debug_type($i));
	assertType("'float'", get_debug_type($f));
	assertType("'float'", get_debug_type($d));
	assertType("'string'", get_debug_type($s));
	assertType("'array'", get_debug_type($a));
	assertType("'stdClass'", get_debug_type($o)); // SKIP: an object built with `new` is not known to be exactly that class
	assertType("string", get_debug_type($std));
	assertType("'GetDebugType\\\\A'", get_debug_type($A));
	assertType("string", get_debug_type($r));
	assertType("string", get_debug_type($resource));
	assertType("'null'", get_debug_type($null));
	assertType("'int'|'string'", get_debug_type($intOrString));
	assertType("'array'|'GetDebugType\\\\A'", get_debug_type($arrayOrObject));
	assertType("'class@anonymous'", get_debug_type($anonymous)); // SKIP: get_debug_type() does not name anonymous classes
	assertType("'GetDebugType\\\\B@anonymous'", get_debug_type($anonymousImplements)); // SKIP: get_debug_type() does not name anonymous classes
	assertType("'GetDebugType\\\\D@anonymous'", get_debug_type($anonymousExtends)); // SKIP: get_debug_type() does not name anonymous classes
	assertType("'GetDebugType\\\\D@anonymous'", get_debug_type($anonymousExtendsImplements)); // SKIP: get_debug_type() does not name anonymous classes
}

/**
 * @param non-empty-string $nonEmptyString
 * @param non-falsy-string $falsyString
 * @param numeric-string $numericString
 * @param class-string $classString
 */
function strings($nonEmptyString, $falsyString, $numericString, $classString) {
	assertType("'string'", get_debug_type($nonEmptyString));
	assertType("'string'", get_debug_type($falsyString));
	assertType("'string'", get_debug_type($numericString));
	assertType("'string'", get_debug_type($classString));
}
