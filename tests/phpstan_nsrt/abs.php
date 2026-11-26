<?php // lint >= 8.0

declare(strict_types = 1);

namespace Abs;

use function PHPStan\Testing\assertType;

class Foo
{

	public function singleIntegerRange(int $int): void
	{
		/** @var int $int */
		assertType('int<0, max>', abs($int));

		/** @var positive-int $int */
		assertType('int<1, max>', abs($int));

		/** @var negative-int $int */
		assertType('int<1, max>', abs($int));

		/** @var non-negative-int $int */
		assertType('int<0, max>', abs($int));

		/** @var non-positive-int $int */
		assertType('int<0, max>', abs($int));

		/** @var int<0, max> $int */
		assertType('int<0, max>', abs($int));

		/** @var int<0, 123> $int */
		assertType('int<0, 123>', abs($int));

		/** @var int<-123, 0> $int */
		assertType('int<0, 123>', abs($int));

		/** @var int<1, max> $int */
		assertType('int<1, max>', abs($int));

		/** @var int<123, max> $int */
		assertType('int<123, max>', abs($int));

		/** @var int<123, 456> $int */
		assertType('int<123, 456>', abs($int));

		/** @var int<min, 0> $int */
		assertType('int<0, max>', abs($int));

		/** @var int<min, -1> $int */
		assertType('int<1, max>', abs($int));

		/** @var int<min, -123> $int */
		assertType('int<123, max>', abs($int));

		/** @var int<-456, -123> $int */
		assertType('int<123, 456>', abs($int));

		/** @var int<-123, 123> $int */
		assertType('int<0, 123>', abs($int));

		/** @var int<min, max> $int */
		assertType('int<0, max>', abs($int));
	}

	public function multipleIntegerRanges(int $int): void
	{
		/** @var non-zero-int $int */
		assertType('int<1, max>', abs($int));

		/** @var int<min, -1>|int<1, max> $int */
		assertType('int<1, max>', abs($int));

		/** @var int<-20, -10>|int<5, 25> $int */
		assertType('int<5, 25>', abs($int));

		/** @var int<-20, -5>|int<10, 25> $int */
		assertType('int<5, 25>', abs($int));

		/** @var int<-25, -10>|int<5, 20> $int */
		assertType('int<5, 25>', abs($int));

		/** @var int<-20, -10>|int<20, 30> $int */
		assertType('int<10, 30>', abs($int));
	}

	public function constantInteger(int $int): void
	{
		/** @var 0 $int */
		assertType('0', abs($int));

		/** @var 1 $int */
		assertType('1', abs($int));

		/** @var -1 $int */
		assertType('1', abs($int));

		assertType('123', abs(123));

		assertType('123', abs(-123));
	}

	public function mixedIntegerUnion(int $int): void
	{
		/** @var 123|int<456, max> $int */
		assertType('123|int<456, max>', abs($int));

		/** @var int<min, -456>|-123 $int */
		assertType('123|int<456, max>', abs($int));

		/** @var -123|int<124, 125> $int */
		assertType('int<123, 125>', abs($int));

		/** @var int<124, 125>|-123 $int */
		assertType('int<123, 125>', abs($int));
	}

	public function constantFloat(float $float): void
	{
		/** @var 0.0 $float */
		assertType('0.0', abs($float));

		/** @var 1.0 $float */
		assertType('1.0', abs($float));

		/** @var -1.0 $float */
		assertType('1.0', abs($float));

		assertType('123.4', abs(123.4));

		assertType('123.4', abs(-123.4));
	}

	public function string(string $string): void
	{
		/** @var string $string */
		assertType('float|int<0, max>', abs($string));

		/** @var numeric-string $string */
		assertType('float|int<0, max>', abs($string));

		/** @var '-1' $string */
		assertType('1', abs($string));

		/** @var '-1'|'-2.0'|'3.0'|'4' $string */
		assertType('1|2.0|3.0|4', abs($string));

		/** @var literal-string $string */
		assertType('float|int<0, max>', abs($string));

		assertType('123', abs('123'));

		assertType('123', abs('-123'));

		assertType('123.0', abs('123.0'));

		assertType('123.0', abs('-123.0'));

		assertType('float|int<0, max>', abs('foo'));
	}

	public function mixedUnion(mixed $value): void
	{
		/** @var 1.0|int<2, 3> $value */
		assertType('1.0|int<2, 3>', abs($value));

		/** @var -1.0|int<-3, -2> $value */
		assertType('1.0|int<2, 3>', abs($value));

		/** @var 2.0|int<1, 3> $value */
		assertType('2.0|int<1, 3>', abs($value));

		/** @var -2.0|int<-3, -1> $value */
		assertType('2.0|int<1, 3>', abs($value));

		/** @var -1.0|int<2, 3>|numeric-string $value */
		assertType('float|int<0, max>', abs($value));
	}

	public function intersection(mixed $value): void
	{
		/** @var int&int<-10, 10> $value */
		assertType('int<0, 10>', abs($value)); // SKIP: an intersection with an integer range is not read
	}

	public function invalidType(mixed $nonInt): void
	{
		/** @var string $nonInt */
		assertType('float|int<0, max>', abs($nonInt));

		/** @var string|positive-int $nonInt */
		assertType('float|int<0, max>', abs($nonInt));

		/** @var 'foo' $nonInt */
		assertType('float|int<0, max>', abs($nonInt));

		/** @var array $nonInt */
		assertType('float|int<0, max>', abs($nonInt));

		/** @var non-empty-list<object> $nonInt */
		assertType('float|int<0, max>', abs($nonInt));

		/** @var object $nonInt */
		assertType('float|int<0, max>', abs($nonInt));

		/** @var \DateTime $nonInt */
		assertType('float|int<0, max>', abs($nonInt));

		/** @var null $nonInt */
		assertType('0', abs($nonInt));

		assertType('float|int<0, max>', abs('foo'));

		assertType('0', abs(null));
	}

}
