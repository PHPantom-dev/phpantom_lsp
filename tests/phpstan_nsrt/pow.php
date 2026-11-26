<?php

namespace PowFunction;

use function PHPStan\Testing\assertType;

function ($a, $b): void {
	assertType('(float|int)', pow($a, $b));
	assertType('(float|int)', $a ** $b);
};

function (int $a, int $b): void {
	assertType('(float|int)', pow($a, $b));
	assertType('(float|int)', $a ** $b);
};

function (\GMP $a, \GMP $b): void {
	assertType('GMP', pow($a, $b));
	assertType('GMP', $a ** $b);
};

function (\stdClass $a, \GMP $b): void {
	assertType('GMP|stdClass', pow($a, $b)); // SKIP: a plain object operand is dropped from an overloaded `**` result
	assertType('GMP|stdClass', $a ** $b); // SKIP: a plain object operand is dropped from an overloaded `**` result

	assertType('GMP|stdClass', pow($b, $a)); // SKIP: a plain object operand is dropped from an overloaded `**` result
	assertType('GMP|stdClass', $b ** $a); // SKIP: a plain object operand is dropped from an overloaded `**` result
};

function (): void {
	$range = rand(1, 3);
	assertType('int<1, 3>', $range); // SKIP: rand() with bounds is typed `int`, not the range between them

	assertType('int<1, 9>', pow($range, 2)); // SKIP: `**` does not work out its result from integer ranges, nor from a 0, 1, `null` or `bool` operand
	assertType('int<1, 9>', $range ** 2); // SKIP: `**` does not work out its result from integer ranges, nor from a 0, 1, `null` or `bool` operand

	assertType('int<2, 8>', pow(2, $range)); // SKIP: `**` does not work out its result from integer ranges, nor from a 0, 1, `null` or `bool` operand
	assertType('int<2, 8>', 2 ** $range); // SKIP: `**` does not work out its result from integer ranges, nor from a 0, 1, `null` or `bool` operand
};

function (): void {
	$range = rand(2, 3);
	$x = 2;
	if (rand(0, 1)) {
		$x = 3;
	} else if (rand(0, 10)) {
		$x = 4;
	}

	assertType('int<4, 81>', pow($range, $x)); // SKIP: `**` does not work out its result from integer ranges, nor from a 0, 1, `null` or `bool` operand
	assertType('int<4, 81>', $range ** $x); // SKIP: `**` does not work out its result from integer ranges, nor from a 0, 1, `null` or `bool` operand

	assertType('int<4, 64>', pow($x, $range)); // SKIP: `**` does not work out its result from integer ranges, nor from a 0, 1, `null` or `bool` operand
	assertType('int<4, 64>', $x ** $range); // SKIP: `**` does not work out its result from integer ranges, nor from a 0, 1, `null` or `bool` operand

	assertType('int<4, 27>', pow($range, $range)); // SKIP: `**` does not work out its result from integer ranges, nor from a 0, 1, `null` or `bool` operand
	assertType('int<4, 27>', $range ** $range); // SKIP: `**` does not work out its result from integer ranges, nor from a 0, 1, `null` or `bool` operand
};

/**
 * @param positive-int $positiveInt
 * @param int<min, 3> $range2
 * @param int<-6, -4>|int<-2, -1> $unionRange1
 * @param int<4, 6>|int<1, 2> $unionRange2
 */
function foo($positiveInt, $range2, $unionRange1, $unionRange2): void {
	$range = rand(2, 3);

	assertType('int<2, max>', pow($range, $positiveInt)); // SKIP: `**` does not work out its result from integer ranges, nor from a 0, 1, `null` or `bool` operand
	assertType('int<2, max>', $range ** $positiveInt); // SKIP: `**` does not work out its result from integer ranges, nor from a 0, 1, `null` or `bool` operand

	assertType('int<min, 27>', pow($range, $range2)); // SKIP: `**` does not work out its result from integer ranges, nor from a 0, 1, `null` or `bool` operand
	assertType('int<min, 27>', $range ** $range2); // SKIP: `**` does not work out its result from integer ranges, nor from a 0, 1, `null` or `bool` operand

	assertType('(float|int)', pow($range, PHP_INT_MAX));
	assertType('(float|int)', $range ** PHP_INT_MAX);

	assertType('(float|int)', pow($range2, $positiveInt));
	assertType('(float|int)', $range2 ** $positiveInt);

	assertType('(float|int)', pow($positiveInt, $range2));
	assertType('(float|int)', $positiveInt ** $range2);

	assertType('int<-6, 16>|int<1296, 4096>', pow($unionRange1, $unionRange2)); // SKIP: `**` does not work out its result from integer ranges, nor from a 0, 1, `null` or `bool` operand
	assertType('int<-6, 16>|int<1296, 4096>', $unionRange1 ** $unionRange2); // SKIP: `**` does not work out its result from integer ranges, nor from a 0, 1, `null` or `bool` operand

	assertType('int<2, 4>|int<16, 64>', pow(2, $unionRange2)); // SKIP: `**` does not work out its result from integer ranges, nor from a 0, 1, `null` or `bool` operand
	assertType('int<2, 4>|int<16, 64>', 2 ** $unionRange2); // SKIP: `**` does not work out its result from integer ranges, nor from a 0, 1, `null` or `bool` operand

	assertType('int<2, 4>|int<16, 64>', pow("2", $unionRange2)); // SKIP: `**` does not work out its result from integer ranges, nor from a 0, 1, `null` or `bool` operand
	assertType('int<2, 4>|int<16, 64>', "2" ** $unionRange2); // SKIP: `**` does not work out its result from integer ranges, nor from a 0, 1, `null` or `bool` operand

	assertType('1', pow(true, $unionRange2)); // SKIP: `**` does not work out its result from integer ranges, nor from a 0, 1, `null` or `bool` operand
	assertType('1', true ** $unionRange2); // SKIP: `**` does not work out its result from integer ranges, nor from a 0, 1, `null` or `bool` operand

	assertType('0|1', pow(null, $unionRange2)); // SKIP: `**` does not work out its result from integer ranges, nor from a 0, 1, `null` or `bool` operand
	assertType('0|1', null ** $unionRange2); // SKIP: `**` does not work out its result from integer ranges, nor from a 0, 1, `null` or `bool` operand
}

/**
 * @param numeric-string $numericS
 */
function doFoo(int $intA, int $intB, string $s, bool $bool, $numericS, float $float, array $arr): void {
	assertType('(float|int)', pow($intA, $intB));
	assertType('(float|int)', $intA ** $intB);

	assertType('(float|int)', pow($intA, $numericS));
	assertType('(float|int)', $intA ** $numericS);
	assertType('(float|int)', $numericS ** $numericS);
	assertType('(float|int)', pow($intA, "123"));
	assertType('(float|int)', $intA ** "123");
	assertType('int', pow($intA, 1)); // SKIP: `**` does not work out its result from integer ranges, nor from a 0, 1, `null` or `bool` operand
	assertType('int', $intA ** '1'); // SKIP: `**` does not work out its result from integer ranges, nor from a 0, 1, `null` or `bool` operand

	assertType('(float|int)', pow($intA, $s));
	assertType('(float|int)', $intA ** $s);

	assertType('(float|int)', pow($intA, $bool)); // could be int
	assertType('(float|int)', $intA ** $bool); // could be int
	assertType('int', pow($intA, true)); // SKIP: `**` does not work out its result from integer ranges, nor from a 0, 1, `null` or `bool` operand
	assertType('int', $intA ** true); // SKIP: `**` does not work out its result from integer ranges, nor from a 0, 1, `null` or `bool` operand

	assertType('*ERROR*', pow($bool, $arr)); // SKIP: an operand `**` rejects is not `*ERROR*`
	assertType('*ERROR*', pow($bool, [])); // SKIP: an operand `**` rejects is not `*ERROR*`

	assertType('0|1', pow(null, "123")); // SKIP: `**` does not work out its result from integer ranges, nor from a 0, 1, `null` or `bool` operand
	assertType('0|1', pow(null, $intA)); // SKIP: `**` does not work out its result from integer ranges, nor from a 0, 1, `null` or `bool` operand
	assertType('1', "123" ** null); // SKIP: `**` does not work out its result from integer ranges, nor from a 0, 1, `null` or `bool` operand
	assertType('1', $intA ** null); // SKIP: `**` does not work out its result from integer ranges, nor from a 0, 1, `null` or `bool` operand
	assertType('1.0', $float ** null); // SKIP: `**` does not work out its result from integer ranges, nor from a 0, 1, `null` or `bool` operand

	assertType('*ERROR*', "123" ** $arr); // SKIP: an operand `**` rejects is not `*ERROR*`
	assertType('*ERROR*', "123" ** []); // SKIP: an operand `**` rejects is not `*ERROR*`

	assertType('625', pow('5', '4'));
	assertType('625', '5' ** '4');

	assertType('(float|int)', pow($intA, $bool)); // could be float
	assertType('(float|int)', $intA ** $bool); // could be float
	assertType('*ERROR*', $intA ** $arr); // SKIP: an operand `**` rejects is not `*ERROR*`
	assertType('*ERROR*', $intA ** []); // SKIP: an operand `**` rejects is not `*ERROR*`

	assertType('1', pow($intA, 0)); // SKIP: `**` does not work out its result from integer ranges, nor from a 0, 1, `null` or `bool` operand
	assertType('1', $intA ** '0'); // SKIP: `**` does not work out its result from integer ranges, nor from a 0, 1, `null` or `bool` operand
	assertType('1', $intA ** false); // SKIP: `**` does not work out its result from integer ranges, nor from a 0, 1, `null` or `bool` operand
	assertType('int', $intA ** true); // SKIP: `**` does not work out its result from integer ranges, nor from a 0, 1, `null` or `bool` operand

	assertType('1.0', pow($float, 0)); // SKIP: `**` does not work out its result from integer ranges, nor from a 0, 1, `null` or `bool` operand
	assertType('1.0', $float ** '0'); // SKIP: `**` does not work out its result from integer ranges, nor from a 0, 1, `null` or `bool` operand
	assertType('1.0', $float ** false); // SKIP: `**` does not work out its result from integer ranges, nor from a 0, 1, `null` or `bool` operand
	assertType('float', pow($float, 1));
	assertType('float', $float ** '1');
	assertType('*ERROR*', $float ** $arr); // SKIP: an operand `**` rejects is not `*ERROR*`
	assertType('*ERROR*', $float ** []); // SKIP: an operand `**` rejects is not `*ERROR*`

	assertType('1.0', pow(1.1, 0));
	assertType('1.0', 1.1 ** '0');
	assertType('1.0', 1.1 ** false); // SKIP: `**` does not work out its result from integer ranges, nor from a 0, 1, `null` or `bool` operand
	assertType('*ERROR*', 1.1 ** $arr); // SKIP: an operand `**` rejects is not `*ERROR*`
	assertType('*ERROR*', 1.1 ** []); // SKIP: an operand `**` rejects is not `*ERROR*`

	assertType('NAN', pow(-1,5.5)); // SKIP: a NAN result is a plain `float`

	assertType('1', pow($s, 0)); // SKIP: `**` does not work out its result from integer ranges, nor from a 0, 1, `null` or `bool` operand
	assertType('1', $s ** '0'); // SKIP: `**` does not work out its result from integer ranges, nor from a 0, 1, `null` or `bool` operand
	assertType('1', $s ** false); // SKIP: `**` does not work out its result from integer ranges, nor from a 0, 1, `null` or `bool` operand
	assertType('(float|int)', pow($s, 1));
	assertType('(float|int)', $s ** '1');
	assertType('*ERROR*', $s ** $arr); // SKIP: an operand `**` rejects is not `*ERROR*`
	assertType('*ERROR*', $s ** []); // SKIP: an operand `**` rejects is not `*ERROR*`

	assertType('1', pow($bool, 0)); // SKIP: `**` does not work out its result from integer ranges, nor from a 0, 1, `null` or `bool` operand
	assertType('1', $bool ** '0'); // SKIP: `**` does not work out its result from integer ranges, nor from a 0, 1, `null` or `bool` operand
	assertType('1', $bool ** false); // SKIP: `**` does not work out its result from integer ranges, nor from a 0, 1, `null` or `bool` operand
	assertType('(float|int)', pow($bool, 1));
	assertType('(float|int)', $bool ** '1');
	assertType('*ERROR*', $bool ** $arr); // SKIP: an operand `**` rejects is not `*ERROR*`
	assertType('*ERROR*', $bool ** []); // SKIP: an operand `**` rejects is not `*ERROR*`
};

function invalidConstantOperands(): void {
	assertType('*ERROR*', 'a' ** 1); // SKIP: an operand `**` rejects is not `*ERROR*`
	assertType('*ERROR*',  1 ** 'a'); // SKIP: an operand `**` rejects is not `*ERROR*`

	assertType('*ERROR*', [] ** 1); // SKIP: an operand `**` rejects is not `*ERROR*`
	assertType('*ERROR*',  1 ** []); // SKIP: an operand `**` rejects is not `*ERROR*`

	assertType('*ERROR*', (new \stdClass()) ** 1); // SKIP: an operand `**` rejects is not `*ERROR*`
	assertType('*ERROR*',  1 ** (new \stdClass())); // SKIP: an operand `**` rejects is not `*ERROR*`
}

function validConstantOperands(): void {
	assertType('1', '1' ** 1);
	assertType('1',  1 ** '1');
	assertType('1',  '1' ** '1');

	assertType('1', true ** 1); // SKIP: `**` does not work out its result from integer ranges, nor from a 0, 1, `null` or `bool` operand
	assertType('1',  1 ** false); // SKIP: `**` does not work out its result from integer ranges, nor from a 0, 1, `null` or `bool` operand
}
