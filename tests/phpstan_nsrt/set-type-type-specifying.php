<?php

namespace SetTypeTypeSpecifying;

use stdClass;
use function PHPStan\Testing\assertType;

function doString(string $s, int $i, float $f, array $a, object $o)
{
	settype($s, 'string');
	assertType('string', $s);

	settype($i, 'string');
	assertType('lowercase-string&numeric-string&uppercase-string', $i); // SKIP: the string an int or float converts to is not known to be upper- or lowercase

	settype($f, 'string');
	assertType('numeric-string&uppercase-string', $f); // SKIP: the string an int or float converts to is not known to be upper- or lowercase

	settype($a, 'string');
	assertType('*ERROR*', $a); // SKIP: a conversion PHP rejects or warns about is not reported as an error type

	settype($o, 'string');
	assertType('*ERROR*', $o); // SKIP: a conversion PHP rejects or warns about is not reported as an error type
}

function doInt(string $s, int $i, float $f, array $a, object $o)
{
	settype($s, 'int');
	assertType('int', $s);

	settype($i, 'int');
	assertType('int', $i);

	settype($f, 'int');
	assertType('int', $f);

	settype($a, 'int');
	assertType('0|1', $a);

	settype($o, 'int');
	assertType('*ERROR*', $o); // SKIP: a conversion PHP rejects or warns about is not reported as an error type
}

function doInteger(string $s, int $i, float $f, array $a, object $o)
{
	settype($s, 'integer');
	assertType('int', $s);

	settype($i, 'integer');
	assertType('int', $i);

	settype($f, 'integer');
	assertType('int', $f);

	settype($a, 'integer');
	assertType('0|1', $a);

	settype($o, 'integer');
	assertType('*ERROR*', $o); // SKIP: a conversion PHP rejects or warns about is not reported as an error type
}

function doFloat(string $s, int $i, float $f, array $a, object $o)
{
	settype($s, 'float');
	assertType('float', $s);

	settype($i, 'float');
	assertType('float', $i);

	settype($f, 'float');
	assertType('float', $f);

	settype($a, 'float');
	assertType('0.0|1.0', $a);

	settype($o, 'float');
	assertType('*ERROR*', $o); // SKIP: a conversion PHP rejects or warns about is not reported as an error type
}

function doDouble(string $s, int $i, float $f, array $a, object $o)
{
	settype($s, 'double');
	assertType('float', $s);

	settype($i, 'double');
	assertType('float', $i);

	settype($f, 'double');
	assertType('float', $f);

	settype($a, 'double');
	assertType('0.0|1.0', $a);

	settype($o, 'double');
	assertType('*ERROR*', $o); // SKIP: a conversion PHP rejects or warns about is not reported as an error type
}

function doBoolean(string $s, int $i, float $f, array $a, object $o)
{
	settype($s, 'bool');
	assertType('bool', $s);

	settype($i, 'bool');
	assertType('bool', $i);

	settype($f, 'bool');
	assertType('bool', $f);

	settype($a, 'bool');
	assertType('bool', $a);

	settype($o, 'bool');
	assertType('true', $o);
}

function doArray(string $s, int $i, float $f, array $a, object $o)
{
	settype($s, 'array');
	assertType('array{string}', $s);

	settype($i, 'array');
	assertType('array{int}', $i);

	settype($f, 'array');
	assertType('array{float}', $f);

	settype($a, 'array');
	assertType('array', $a);

	settype($o, 'array');
	assertType('array', $o);
}

function doObject(string $s, int $i, float $f, array $a, object $o)
{
	settype($s, 'object');
	// PHPantom keeps what the cast reads from a scalar or array, where PHPStan says `stdClass`.
	assertType('object{scalar: string}&stdClass', $s);

	settype($i, 'object');
	// PHPantom keeps what the cast reads from a scalar or array, where PHPStan says `stdClass`.
	assertType('object{scalar: int}&stdClass', $i);

	settype($f, 'object');
	// PHPantom keeps what the cast reads from a scalar or array, where PHPStan says `stdClass`.
	assertType('object{scalar: float}&stdClass', $f);

	settype($a, 'object');
	assertType('stdClass', $a);

	settype($o, 'object');
	assertType('stdClass', $o);
}

function doNull(string $s, int $i, float $f, array $a, object $o)
{
	settype($s, 'null');
	assertType('null', $s);

	settype($i, 'null');
	assertType('null', $i);

	settype($f, 'null');
	assertType('null', $f);

	settype($a, 'null');
	assertType('null', $a);

	settype($o, 'null');
	assertType('null', $o);
}

/**
 * @param 'string'|'int'|'integer'|'float'|'double'|'bool'|'boolean'|'array'|'object'|'null' $castTo
 */
function setTypeSpecifying($value, string $castTo): void
{
	// String to constant string
	$x = 'some-string';
	settype($x, 'string');
	assertType("'some-string'", $x);

	// String to constant int
	$x = 'some-string';
	settype($x, 'int');
	assertType("0", $x);

	// String to constant integer
	$x = 'some-string';
	settype($x, 'integer');
	assertType("0", $x);

	// String to constant float
	$x = 'some-string';
	settype($x, 'float');
	assertType('0.0', $x);

	// String to constant double
	$x = 'some-string';
	settype($x, 'double');
	assertType('0.0', $x);

	// String to constant bool.
	$x = 'some-string';
	settype($x, 'bool');
	assertType('true', $x);

	// String to constant boolean.
	$x = 'some-string';
	settype($x, 'boolean');
	assertType('true', $x);

	// String to constant array
	$x = 'some-string';
	settype($x, 'array');
	assertType("array{'some-string'}", $x);

	// String to constant object
	$x = 'some-string';
	settype($x, 'object');
	// PHPantom keeps what the cast reads from a scalar or array, where PHPStan says `stdClass`.
	assertType('object{scalar: string}&stdClass', $x);

	// String to constant null
	$x = 'some-string';
	settype($x, 'null');
	assertType('null', $x);

	// String to non-constant.
	$x = 'some-string';
	settype($x, $castTo);
	// PHPantom keeps what the cast reads from a scalar or array, where PHPStan says `stdClass`.
	assertType("'some-string'|0|0.0|true|array{'some-string'}|(object{scalar: string}&stdClass)|null", $x);

	// int to string
	$x = 123;
	settype($x, 'string');
	assertType("'123'", $x);

	// int to int
	$x = 123;
	settype($x, 'int');
	assertType('123', $x);

	// int to integer
	$x = 123;
	settype($x, 'integer');
	assertType('123', $x);

	// int to float
	$x = 123;
	settype($x, 'float');
	assertType('123.0', $x);

	// int to double
	$x = 123;
	settype($x, 'double');
	assertType('123.0', $x);

	// int to bool
	$x = 123;
	settype($x, 'bool');
	assertType('true', $x);

	// int 0 to bool
	$x = 0;
	settype($x, 'bool');
	assertType('false', $x);

	// int to boolean
	$x = 123;
	settype($x, 'boolean');
	assertType('true', $x);

	// int to array
	$x = 123;
	settype($x, 'array');
	assertType('array{123}', $x);

	// int to object
	$x = 123;
	settype($x, 'object');
	// PHPantom keeps what the cast reads from a scalar or array, where PHPStan says `stdClass`.
	assertType('object{scalar: int}&stdClass', $x);

	// int to null
	$x = 123;
	settype($x, 'null');
	assertType('null', $x);

	// float to string
	$x = 123.0;
	settype($x, 'string');
	assertType("'123'", $x);

	// float to int
	$x = 123.0;
	settype($x, 'int');
	assertType('123', $x);

	// float to integer
	$x = 123.0;
	settype($x, 'integer');
	assertType('123', $x);

	// float to float
	$x = 123.0;
	settype($x, 'float');
	assertType('123.0', $x);

	// float to double
	$x = 123.0;
	settype($x, 'double');
	assertType('123.0', $x);

	// float to bool
	$x = 123.0;
	settype($x, 'bool');
	assertType('true', $x);

	// float 0.0 to bool
	$x = 0.0;
	settype($x, 'bool');
	assertType('false', $x);

	// float to boolean
	$x = 123.0;
	settype($x, 'boolean');
	assertType('true', $x);

	// float to array
	$x = 123.0;
	settype($x, 'array');
	assertType('array{123.0}', $x);

	// float to object
	$x = 123;
	settype($x, 'object');
	// PHPantom keeps what the cast reads from a scalar or array, where PHPStan says `stdClass`.
	assertType('object{scalar: int}&stdClass', $x);

	// float to null
	$x = 123;
	settype($x, 'null');
	assertType('null', $x);

	// bool true to string
	$x = true;
	settype($x, 'string');
	assertType("'1'", $x);

	// bool false to string
	$x = false;
	settype($x, 'string');
	assertType("''", $x);

	// bool true to int
	$x = true;
	settype($x, 'int');
	assertType('1', $x);

	// bool false to int
	$x = false;
	settype($x, 'int');
	assertType('0', $x);

	// bool true to integer
	$x = true;
	settype($x, 'integer');
	assertType('1', $x);

	// bool false to integer
	$x = false;
	settype($x, 'integer');
	assertType('0', $x);

	// bool true to float
	$x = true;
	settype($x, 'float');
	assertType('1.0', $x);

	// bool false to float
	$x = false;
	settype($x, 'float');
	assertType('0.0', $x);

	// bool true to double
	$x = true;
	settype($x, 'double');
	assertType('1.0', $x);

	// bool false to double
	$x = false;
	settype($x, 'double');
	assertType('0.0', $x);

	// bool true to bool
	$x = true;
	settype($x, 'bool');
	assertType('true', $x);

	// bool false to bool
	$x = false;
	settype($x, 'bool');
	assertType('false', $x);

	// bool true to boolean
	$x = true;
	settype($x, 'boolean');
	assertType('true', $x);

	// bool false to boolean
	$x = false;
	settype($x, 'boolean');
	assertType('false', $x);

	// bool true to array
	$x = true;
	settype($x, 'array');
	assertType('array{true}', $x);

	// bool false to array
	$x = false;
	settype($x, 'array');
	assertType('array{false}', $x);

	// bool true to object
	$x = true;
	settype($x, 'object');
	// PHPantom keeps what the cast reads from a scalar or array, where PHPStan says `stdClass`.
	assertType('object{scalar: true}&stdClass', $x);

	// bool false to object
	$x = false;
	settype($x, 'object');
	// PHPantom keeps what the cast reads from a scalar or array, where PHPStan says `stdClass`.
	assertType('object{scalar: false}&stdClass', $x);

	// bool true to null
	$x = true;
	settype($x, 'null');
	assertType('null', $x);

	// bool false to null
	$x = false;
	settype($x, 'null');
	assertType('null', $x);

	//array to string
	$x = [];
	settype($x, 'string');
	assertType('*ERROR*', $x); // SKIP: a conversion PHP rejects or warns about is not reported as an error type

	$x = ['foo'];
	settype($x, 'string');
	assertType('*ERROR*', $x); // SKIP: a conversion PHP rejects or warns about is not reported as an error type

	$x = ['foo' => 'bar'];
	settype($x, 'string');
	assertType('*ERROR*', $x); // SKIP: a conversion PHP rejects or warns about is not reported as an error type

	// array to int
	$x = [];
	settype($x, 'int');
	assertType('0', $x);

	$x = ['foo'];
	settype($x, 'int');
	assertType('1', $x);

	$x = ['foo' => 'bar'];
	settype($x, 'int');
	assertType('1', $x);

	// array to integer
	$x = [];
	settype($x, 'integer');
	assertType('0', $x);

	$x = ['foo'];
	settype($x, 'integer');
	assertType('1', $x);

	$x = ['foo' => 'bar'];
	settype($x, 'integer');
	assertType('1', $x);

	// array to float
	$x = [];
	settype($x, 'float');
	assertType('0.0', $x);

	$x = ['foo'];
	settype($x, 'float');
	assertType('1.0', $x);

	$x = ['foo' => 'bar'];
	settype($x, 'float');
	assertType('1.0', $x);

	// array to double
	$x = [];
	settype($x, 'double');
	assertType('0.0', $x);

	$x = ['foo'];
	settype($x, 'double');
	assertType('1.0', $x);

	$x = ['foo' => 'bar'];
	settype($x, 'double');
	assertType('1.0', $x);

	// array to bool
	$x = [];
	settype($x, 'bool');
	assertType('false', $x);

	$x = ['foo'];
	settype($x, 'bool');
	assertType('true', $x);

	$x = ['foo' => 'bar'];
	settype($x, 'bool');
	assertType('true', $x);

	// array to boolean
	$x = [];
	settype($x, 'boolean');
	assertType('false', $x);

	$x = ['foo'];
	settype($x, 'boolean');
	assertType('true', $x);

	$x = ['foo' => 'bar'];
	settype($x, 'boolean');
	assertType('true', $x);

	// array to array
	$x = [];
	settype($x, 'array');
	assertType('array{}', $x);

	$x = ['foo'];
	settype($x, 'array');
	assertType("array{'foo'}", $x);

	$x = ['foo' => 'bar'];
	settype($x, 'array');
	assertType("array{foo: 'bar'}", $x);

	// array to object
	$x = [];
	settype($x, 'object');
	assertType('stdClass', $x);

	$x = ['foo'];
	settype($x, 'object');
	// PHPantom keeps what the cast reads from a scalar or array, where PHPStan says `stdClass`.
	assertType("object{'foo'}&stdClass", $x);

	$x = ['foo' => 'bar'];
	settype($x, 'object');
	// PHPantom keeps what the cast reads from a scalar or array, where PHPStan says `stdClass`.
	assertType("object{foo: 'bar'}&stdClass", $x);

	// array to null
	$x = [];
	settype($x, 'null');
	assertType('null', $x);

	$x = ['foo'];
	settype($x, 'null');
	assertType('null', $x);

	$x = ['foo' => 'bar'];
	settype($x, 'null');
	assertType('null', $x);

	// object to string
	$x = new stdClass();
	settype($x, 'string');
	assertType('*ERROR*', $x); // SKIP: a conversion PHP rejects or warns about is not reported as an error type

	// object to int
	$x = new stdClass();
	settype($x, 'int');
	assertType('*ERROR*', $x); // SKIP: a conversion PHP rejects or warns about is not reported as an error type

	// object to integer
	$x = new stdClass();
	settype($x, 'integer');
	assertType('*ERROR*', $x); // SKIP: a conversion PHP rejects or warns about is not reported as an error type

	// object to float
	$x = new stdClass();
	settype($x, 'float');
	assertType('*ERROR*', $x); // SKIP: a conversion PHP rejects or warns about is not reported as an error type

	// object to double
	$x = new stdClass();
	settype($x, 'double');
	assertType('*ERROR*', $x); // SKIP: a conversion PHP rejects or warns about is not reported as an error type

	// object to bool
	$x = new stdClass();
	settype($x, 'bool');
	assertType('true', $x);

	// object to boolean
	$x = new stdClass();
	settype($x, 'boolean');
	assertType('true', $x);

	// object to array
	$x = new stdClass();
	settype($x, 'array');
	assertType('array', $x);

	// object to object
	$x = new stdClass();
	settype($x, 'object');
	assertType('stdClass', $x);

	// object to null
	$x = new stdClass();
	settype($x, 'null');
	assertType('null', $x);

	// null to string
	$x = null;
	settype($x, 'string');
	assertType("''", $x);

	// null to int
	$x = null;
	settype($x, 'int');
	assertType('0', $x);

	// null to integer
	$x = null;
	settype($x, 'integer');
	assertType('0', $x);

	// null to float
	$x = null;
	settype($x, 'float');
	assertType('0.0', $x);

	// null to double
	$x = null;
	settype($x, 'double');
	assertType('0.0', $x);

	// null to bool
	$x = null;
	settype($x, 'bool');
	assertType('false', $x);

	// null to boolean
	$x = null;
	settype($x, 'boolean');
	assertType('false', $x);

	// null to array
	$x = null;
	settype($x, 'array');
	assertType('array{}', $x);

	// null to object
	$x = null;
	settype($x, 'object');
	assertType('stdClass', $x);

	// null to null
	$x = null;
	settype($x, 'null');
	assertType('null', $x);

	// Mixed to non-constant.
	settype($value, $castTo);
	assertType("array|bool|float|int|stdClass|string|null", $value);
}
