<?php

namespace Bug6901;

use function PHPStan\Testing\assertType;

/**
 * @param integer|string|array<string>|bool $y
 * @return integer
 */
function foo($y)
{
	switch (gettype($y)) {
		case "integer":
			assertType('int', $y);
			break;
		case "string":
			assertType('string', $y);
			break;
		case "boolean":
			assertType('bool', $y);
			break;
		case "array":
			assertType('array<string>', $y);
			break;
		default:
			assertType('*NEVER*', $y);
	}
	assertType('array<string>|bool|int|string', $y);
	return 0;
}

/**
 * @param object|float|null|resource $y
 * @return integer
 */
function bar($y)
{
	switch (gettype($y)) {
		case "object":
			assertType('object', $y);
			break;
		case "double":
			assertType('float', $y);
			break;
		case "NULL":
			assertType('null', $y);
			break;
		case "resource":
			assertType('resource', $y);
			break;
		default:
			assertType('resource', $y); // PHPStan says `*NEVER*`, but gettype() of a closed resource is 'resource (closed)'
	}
	assertType('float|object|resource|null', $y);
	return 0;
}

/**
 * @param int|string|bool $x
 * @param int|string|bool $y
 */
function foobarIdentical($x, $y)
{
	if (gettype($x) === 'integer') {
		assertType('int', $x);
		return;
	}
	assertType('bool|string', $x);

	if ('boolean' === gettype($x)) {
		assertType('bool', $x);
		return;
	}

	if (gettype($y) === 'string' || gettype($y) === 'integer') {
		assertType('int|string', $y);
	}
}

/**
 * @param int|string|bool $x
 */
function foobarEqual($x)
{
	if (gettype($x) == 'integer') {
		assertType('int', $x);
		return;
	}

	if ('boolean' == gettype($x)) {
		assertType('bool', $x);
		return;
	}
	
	assertType('string', $x);
}
