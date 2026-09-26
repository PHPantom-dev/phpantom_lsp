<?php

namespace BinaryOperations\NestedNamespace;

use function PHPStan\Testing\assertType;

class Foo
{

	public const INT_CONST = 1;

	public function doFoo()
	{
		/** @var float $float */
		$float = doFoo();
		$float1 = $float;
		$float2 = $float;
		$float3 = $float;
		$float4 = $float;
		$float5 = $float;
		$float6 = $float;

		/** @var int $integer */
		$integer = doFoo();
		$integer1 = $integer;
		$integer2 = $integer;
		$integer3 = $integer;
		$integer4 = $integer;
		$integer5 = $integer;
		$integer6 = $integer;
		$integer7 = $integer;
		$integer8 = $integer;

		/** @var bool $bool */
		$bool = doFoo();

		/** @var string $string */
		$string = doFoo();
		$string1 = $string;
		$string2 = $string;
		$string3 = $string;
		$string4 = $string;

		/** @var string|null $stringOrNull */
		$stringOrNull = doFoo();

		/** @var number $number */
		$number = doFoo();

		/** @var int|null|bool $otherInteger */
		$otherInteger = doFoo();

		/** @var mixed $mixed */
		$mixed = doFoo();

		/** @var int|float $intOrFloat */
		$intOrFloat = doFoo();

		/** @var int|array $intOrArray */
		$intOrArray = doFoo();

		/** @var array|float $floatOrArray */
		$floatOrArray = doFoo();

		// ── Unary operators ─────────────────────────────────────────────

		assertType('int', -$integer);
		assertType('int', +$integer);
		assertType('float', -$float);
		assertType('-1', -1);

		// ── Arithmetic: literal int op int ──────────────────────────────

		assertType('2', 1 + 1);
		assertType('0', 1 - 1);
		assertType('0.5', 1 / 2);
		assertType('1', 1 * 1);
		assertType('1', 1 ** 1);
		assertType('0', 1 % 1);

		// ── Arithmetic: literal float op float ──────────────────────────

		assertType('2.5999999999999996', 1.2 + 1.4);
		assertType('-0.19999999999999996', 1.2 - 1.4);
		assertType('0.5', 1.2 / 2.4);
		assertType('1.68', 1.2 * 1.4);
		assertType('1.290784508319084', 1.2 ** 1.4);
		assertType('int', 3.2 % 2.4);

		// ── Arithmetic: literal int op float ────────────────────────────

		assertType('2.4', 1 + 1.4);
		assertType('-0.3999999999999999', 1 - 1.4);
		assertType('0.4166666666666667', 1 / 2.4);
		assertType('1.4', 1 * 1.4);
		assertType('1.0', 1 ** 1.4);
		assertType('int', 3 % 2.4);

		// ── Arithmetic: literal float op int ────────────────────────────

		assertType('2.2', 1.2 + 1);
		assertType('0.19999999999999996', 1.2 - 1);
		assertType('0.6', 1.2 / 2);
		assertType('1.2', 1.2 * 1);
		assertType('1.2', 1.2 ** 1);
		assertType('int', 3.2 % 2);

		// ── Arithmetic: variable int operations ─────────────────────────

		assertType('int', $integer * 10);
		assertType('int|float', $integer ** $integer);
		assertType('int|float', $integer / $integer);
		assertType('int', $otherInteger + 1);
		assertType('float', $otherInteger + 1.0);

		// ── Arithmetic: variable float operations ───────────────────────

		assertType('float', $float + $float);
		assertType('int|float', $float + $number);

		// ── Arithmetic: number type ─────────────────────────────────────

		assertType('int|float', 1 + $number);
		assertType('int|float', $integer + $number);
		assertType('int|float', 1 / $number);
		assertType('int|float', 1.0 / $number);
		assertType('int|float', $number / 1);
		assertType('int|float', $number / 1.0);
		assertType('int|float', 1.0 + $number);
		assertType('int|float', $number + 1);
		assertType('int|float', $number + 1.0);

		// ── Arithmetic: mixed operands ──────────────────────────────────

		assertType('int|float', 1.0 / $mixed);
		assertType('int|float', $mixed / 1.0);
		assertType('int|float', 1.0 + $mixed);
		assertType('int|float', $mixed + 1.0);

		// ── Arithmetic: union type operands ─────────────────────────────

		assertType('int|float', $intOrFloat + $intOrFloat);

		// ── Arithmetic: bool operands ───────────────────────────────────

		assertType('int', true + false);

		// ── String concatenation ────────────────────────────────────────

		assertType('string', $string . $string);
		assertType('string', $string . 'foo');
		assertType('string', 'foo' . $string);
		assertType('string', $string . $integer);
		assertType('string', $integer . $string);

		// ── Comparison operators ────────────────────────────────────────

		assertType('bool', $string === "foo");
		assertType('bool', $string !== "foo");
		assertType('bool', $string == "foo");
		assertType('bool', $string != "foo");
		assertType('bool', $integer > 0);
		assertType('bool', $integer >= 0);
		assertType('bool', $integer < 0);
		assertType('bool', $integer <= 0);

		// ── Spaceship ───────────────────────────────────────────────────

		assertType('1', 'foo' <=> 'bar');

		// ── Bitwise operators: int operands ─────────────────────────────

		assertType('1', 5 & 3);
		assertType('int', $integer & 3);
		assertType('int', $integer & $integer);
		assertType('7', 5 | 3);
		assertType('int', $integer | 3);
		assertType('6', 5 ^ 3);
		assertType('int', $integer ^ 3);

		// ── Bitwise: string operands ────────────────────────────────────

		assertType('string', "x" & "y");
		assertType('string', $string & "x");
		assertType('string', "x" | "y");
		assertType('string', $string | "x");
		assertType('string', "x" ^ "y");
		assertType('string', $string ^ "x");



		// ── Null coalescing ─────────────────────────────────────────────

		assertType('string', $string ?? 'foo');
		assertType('string', $stringOrNull ?? 'foo');
		assertType('string|int', $string ?? $integer);
		assertType('int|string', $stringOrNull ?? $integer);
		assertType('string|null', $stringOrNull ?? null);

		// ── Ternary ─────────────────────────────────────────────────────

		assertType('1', true ? 1 : 2);
		assertType('2', false ? 1 : 2);

		// ── instanceof ──────────────────────────────────────────────────

		$foo = new Foo();
		assertType('bool', $foo instanceof \BinaryOperations\NestedNamespace\Foo);

		// ── Logical operators ───────────────────────────────────────────
		// `xor`, `and`, `or` have lower precedence than `=` so they
		// cannot be tested via assertType (the assignment captures the
		// LHS before the logical operator runs).

		assertType('bool', true && false);
		assertType('bool', true || false);
		assertType('bool', !true);

		// ── Compound assignment ─────────────────────────────────────────
		// assertType wraps as `$__assert = $x /= 2`, which the hover
		// path inside class methods does not resolve (compound assignment
		// as RHS).  These work at namespace level via the forward walker.

		assertType('int|float', $integer1 /= 2);
		assertType('int', $integer2 *= 1);
		assertType('float', $float1 /= 2.4);
		assertType('float', $float2 *= 2.4);
		assertType('float', $integer3 /= 2.4);
		assertType('float', $integer4 *= 2.4);
		assertType('int', $float3 %= 2.4);
		assertType('float', $float4 **= 2.4);
		assertType('float', $float5 /= 2.4);
		assertType('float', $float6 *= 2);
		assertType('int', $integer5 <<= 2.2);
		assertType('int', $integer6 &= 3);
		assertType('int', $integer7 |= 3);
		assertType('int', $integer8 ^= 3);

		// ── Mixed arithmetic with mixed ─────────────────────────────────

		assertType('float|int', 1 + $mixed);
		assertType('float|int', 1 / $mixed);
		assertType('float|int', $mixed / 1);
		assertType('float|int', $mixed + 1);
	}

}
