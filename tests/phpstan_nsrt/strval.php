<?php

namespace StrvalFamilyTest;

use function PHPStan\Testing\assertType;

/**
 * @param class-string<\stdClass> $class
 */
function strvalTest(string $string, string $class): void
{
	assertType('null', strval()); // SKIP: a call with no argument is an error, not `null`
	assertType('\'foo\'', strval('foo'));
	assertType('string', strval($string));
	assertType('\'\'', strval(null));
	assertType('\'\'', strval(false));
	assertType('\'1\'', strval(true));
	assertType('\'\'|\'1\'', strval(rand(0, 1) === 0));
	assertType('\'42\'', strval(42));
	assertType('lowercase-string&numeric-string&uppercase-string', strval(rand())); // SKIP: a number converted to a string is not typed `lowercase-string&uppercase-string`
	assertType('numeric-string&uppercase-string', strval(rand() * 0.1)); // SKIP: a number converted to a string is not typed `lowercase-string&uppercase-string`
	assertType('lowercase-string&numeric-string&uppercase-string', strval(strval(rand()))); // SKIP: a number converted to a string is not typed `lowercase-string&uppercase-string`
	assertType('class-string<stdClass>', strval($class));
	assertType('string', strval(new \Exception()));
	assertType('*ERROR*', strval(new \stdClass())); // SKIP: a value PHP cannot convert is not `*ERROR*`
	assertType('*ERROR*', strval([])); // SKIP: a value PHP cannot convert is not `*ERROR*`
	assertType('*ERROR*', strval(function() {})); // SKIP: a value PHP cannot convert is not `*ERROR*`
	assertType('string', strval(fopen('php://memory', 'r')));
}

function intvalTest(string $string): void
{
	assertType('null', intval()); // SKIP: a call with no argument is an error, not `null`
	assertType('42', intval('42'));
	assertType('0', intval('foo'));
	assertType('int', intval($string));
	assertType('0', intval(null));
	assertType('0', intval(false));
	assertType('1', intval(true));
	assertType('0|1', intval(rand(0, 1) === 0)); // SKIP: like a cast, a conversion of an operand that is not a literal, variable or constant is not resolved
	assertType('42', intval(42));
	assertType('int<0, max>', intval(rand())); // SKIP: like a cast, a conversion of an operand that is not a literal, variable or constant is not resolved
	assertType('int', intval(rand() * 0.1));
	assertType('0', intval([])); // SKIP: like a cast, a conversion of an operand that is not a literal, variable or constant is not resolved
	assertType('1', intval([null])); // SKIP: like a cast, a conversion of an operand that is not a literal, variable or constant is not resolved
	assertType('int', intval(new \stdClass()));
	assertType('int', intval(function() {}));
	assertType('int', intval(fopen('php://memory', 'r')));
}

function floatvalTest(string $string): void
{
	assertType('null', floatval()); // SKIP: a call with no argument is an error, not `null`
	assertType('3.14', floatval('3.14'));
	assertType('0.0', floatval('foo'));
	assertType('float', floatval($string));
	assertType('0.0', floatval(null));
	assertType('0.0', floatval(false));
	assertType('1.0', floatval(true));
	assertType('0.0|1.0', floatval(rand(0, 1) === 0)); // SKIP: like a cast, a conversion of an operand that is not a literal, variable or constant is not resolved
	assertType('42.0', floatval(42));
	assertType('float', floatval(rand()));
	assertType('float', floatval(rand() * 0.1));
	assertType('0.0', floatval([])); // SKIP: like a cast, a conversion of an operand that is not a literal, variable or constant is not resolved
	assertType('1.0', floatval([null])); // SKIP: like a cast, a conversion of an operand that is not a literal, variable or constant is not resolved
	assertType('float', floatval(new \stdClass()));
	assertType('float', floatval(function() {}));
	assertType('float', floatval(fopen('php://memory', 'r')));
}

function boolvalTest(string $string): void
{
	assertType('null', boolval()); // SKIP: a call with no argument is an error, not `null`
	assertType('false', boolval(''));
	assertType('true', boolval('foo'));
	assertType('bool', boolval($string));
	assertType('false', boolval(null));
	assertType('false', boolval(false));
	assertType('true', boolval(true));
	assertType('bool', boolval(rand(0, 1) === 0));
	assertType('true', boolval(42));
	assertType('bool', boolval(rand()));
	assertType('bool', boolval(rand() * 0.1));
	assertType('false', boolval([])); // SKIP: like a cast, a conversion of an operand that is not a literal, variable or constant is not resolved
	assertType('true', boolval([null])); // SKIP: like a cast, a conversion of an operand that is not a literal, variable or constant is not resolved
	assertType('true', boolval(new \stdClass())); // SKIP: like a cast, a conversion of an operand that is not a literal, variable or constant is not resolved
	assertType('true', boolval(function() {})); // SKIP: like a cast, a conversion of an operand that is not a literal, variable or constant is not resolved
	assertType('bool', boolval(fopen('php://memory', 'r')));
}

function arrayTest(array $a): void
{
	assertType('0|1', intval($a));
	assertType('0.0|1.0', floatval($a));
	assertType('bool', boolval($a));
}

/** @param non-empty-array $a */
function nonEmptyArrayTest(array $a): void
{
	assertType('1', intval($a));
	assertType('1.0', floatval($a));
	assertType('true', boolval($a));
}

/**
 * @param array{} $a
 * @param array{foo: mixed, bar?: mixed} $b
 * @param array{foo?: mixed, bar?: mixed} $c
 */
function constantArrayTest(array $a, array $b, array $c): void
{
	assertType('0', intval($a));
	assertType('0.0', floatval($a));
	assertType('false', boolval($a));

	assertType('1', intval($b));
	assertType('1.0', floatval($b));
	assertType('true', boolval($b));

	assertType('0|1', intval($c));
	assertType('0.0|1.0', floatval($c));
	assertType('bool', boolval($c));
}
