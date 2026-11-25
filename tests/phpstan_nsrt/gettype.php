<?php

namespace GettypeFunction;

use function PHPStan\Testing\assertType;

class A {}

/**
 * @param double $d
 * @param resource $r
 * @param int|string $intOrString
 * @param array|object $arrayOrObject
 */
function doFoo(bool $b, int $i, float $f, $d, $r, string $s, array $a, $mixed, $intOrString, $arrayOrObject) {
	$null = null;
	$resource = fopen('php://memory', 'r');
	$o = new \stdClass();
	$arrayObject = new \ArrayObject();
	$A = new A();

	assertType("'boolean'", gettype($b));
	assertType("'boolean'", gettype(true));
	assertType("'boolean'", gettype(false));
	assertType("'integer'", gettype($i));
	assertType("'double'", gettype($f));
	assertType("'double'", gettype($d));
	assertType("'string'", gettype($s));
	assertType("'array'", gettype($a));
	assertType("'object'", gettype($o));
	assertType("'object'", gettype($arrayObject));
	assertType("'object'", gettype($A));
	// 'closed' was added in php 7.2
	assertType("'resource'|'resource (closed)'", gettype($r));
	assertType("'boolean'|'resource'|'resource (closed)'", gettype($resource));
	assertType("'NULL'", gettype($null));
	assertType("'array'|'boolean'|'double'|'integer'|'NULL'|'object'|'resource'|'resource (closed)'|'string'|'unknown type'", gettype($mixed)); // SKIP: an unannotated parameter has no type to name

	assertType("'integer'|'string'", gettype($intOrString));
	assertType("'array'|'object'", gettype($arrayOrObject));
}

/**
 * @param non-empty-string $nonEmptyString
 * @param non-falsy-string $falsyString
 * @param numeric-string $numericString
 * @param class-string $classString
 */
function strings($nonEmptyString, $falsyString, $numericString, $classString) {
	assertType("'string'", gettype($nonEmptyString));
	assertType("'string'", gettype($falsyString));
	assertType("'string'", gettype($numericString));
	assertType("'string'", gettype($classString));
}
