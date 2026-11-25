<?php

namespace ComparisonOperators;

use function PHPStan\Testing\assertType;

class ComparisonOperators
{
	public function null(): void
	{

	}

	public function bool(): void
	{
	}

	public function string(): void
	{
	}

	public function float(): void
	{
		assertType('true', 1.9 > 1);

		assertType('false', 1.9 > 2.1);
		assertType('true', 1.9 > 1.5);
		assertType('true', 1.9 < 2.1);
		assertType('false', 1.9 < 1.5);
	}

	public function unions(int $a, int $b): void
	{
		if (($a === 17 || $a === 42) && ($b === 3 || $b === 7)) {
		}
		if (($a === 11 || $a === 42) && ($b === 3 || $b === 11)) {
			assertType('bool', $a > $b);
			assertType('bool', $a <= $b);
		}
	}

	public function ranges(int $a, int $b): void
	{
		if ($a >= 10 && $a <= 20) {
			if ($b >= 30 && $b <= 40) {
			}
		}
		if ($a >= 10 && $a <= 25) {
			if ($b >= 25 && $b <= 40) {
				assertType('bool', $a < $b);
				assertType('bool', $a >= $b);
			}
		}
	}
}

class ComparisonOperatorsInTypeSpecifier
{

	public function null(?int $i, ?float $f, ?string $s, ?bool $b): void
	{
		if ($i > null) {
		}
		if ($i >= null) {
			assertType('int|null', $i);
		}
		if ($i < null) {
			assertType('*NEVER*', $i); // SKIP: a branch that cannot run keeps the type instead of `*NEVER*`
		}
		if ($i <= null) {
			assertType('0|null', $i);
		}

		if ($f > null) {
			assertType('float', $f);
		}
		if ($f >= null) {
			assertType('float|null', $f);
		}
		if ($f < null) {
			assertType('*NEVER*', $f); // SKIP: a branch that cannot run keeps the type instead of `*NEVER*`
		}
		if ($f <= null) {
			assertType('0.0|null', $f);
		}

		if ($s > null) {
		}
		if ($s >= null) {
			assertType('string|null', $s);
		}
		if ($s < null) {
			assertType('*NEVER*', $s); // SKIP: a branch that cannot run keeps the type instead of `*NEVER*`
		}
		if ($s <= null) {
			assertType('\'\'|null', $s);
		}

		if ($b > null) {
		}
		if ($b >= null) {
			assertType('bool|null', $b);
		}
		if ($b < null) {
			assertType('*NEVER*', $b); // SKIP: a branch that cannot run keeps the type instead of `*NEVER*`
		}
		if ($b <= null) {
			assertType('false|null', $b);
		}
	}

	public function bool(?bool $b): void
	{
		if ($b > false) {
		}
		if ($b >= false) {
			assertType('bool|null', $b);
		}
		if ($b < false) {
			assertType('*NEVER*', $b); // SKIP: a branch that cannot run keeps the type instead of `*NEVER*`
		}
		if ($b <= false) {
		}

		if ($b > true) {
			assertType('*NEVER*', $b); // SKIP: a branch that cannot run keeps the type instead of `*NEVER*`
		}
		if ($b >= true) {
		}
		if ($b < true) {
		}
		if ($b <= true) {
			assertType('bool|null', $b);
		}
	}

	public function string(?string $s): void
	{
		if ($s < '') {
			assertType('*NEVER*', $s); // SKIP: a branch that cannot run keeps the type instead of `*NEVER*`
		}
		if ($s <= '') {
			assertType('string|null', $s); // Would be nice to have ''|null
		}
	}

	public function intPositive10(?int $i, ?float $f): void
	{
		if ($i > 10) {
			assertType('int<11, max>', $i);
		}
		if ($i >= 10) {
			assertType('int<10, max>', $i);
		}
		if ($i < 10) {
			assertType('int<min, 9>|null', $i);
		}
		if ($i <= 10) {
			assertType('int<min, 10>|null', $i);
		}

		if ($f > 10) {
			assertType('float', $f);
		}
		if ($f >= 10) {
			assertType('float', $f);
		}
		if ($f < 10) {
			assertType('float|null', $f);
		}
		if ($f <= 10) {
			assertType('float|null', $f);
		}
	}

	public function intNegative10(?int $i, ?float $f): void
	{
		if ($i > -10) {
			assertType('int<-9, max>', $i);
		}
		if ($i >= -10) {
			assertType('int<-10, max>', $i);
		}
		if ($i < -10) {
			assertType('int<min, -11>|null', $i);
		}
		if ($i <= -10) {
			assertType('int<min, -10>|null', $i);
		}

		if ($f > -10) {
			assertType('float', $f);
		}
		if ($f >= -10) {
			assertType('float', $f);
		}
		if ($f < -10) {
			assertType('float|null', $f);
		}
		if ($f <= -10) {
			assertType('float|null', $f);
		}
	}

	public function intZero(?int $i, ?float $f): void
	{
		if ($i > 0) {
			assertType('int<1, max>', $i);
		}
		if ($i >= 0) {
		}
		if ($i < 0) {
			assertType('int<min, -1>', $i);
		}
		if ($i <= 0) {
			assertType('int<min, 0>|null', $i);
		}

		if ($f > 0) {
			assertType('float', $f);
		}
		if ($f >= 0) {
			assertType('float|null', $f);
		}
		if ($f < 0) {
			assertType('float', $f);
		}
		if ($f <= 0) {
			assertType('float|null', $f);
		}
	}

	public function float10(?int $i): void
	{
		if ($i > 10.0) {
		}
		if ($i >= 10.0) {
		}
		if ($i < 10.0) {
		}
		if ($i <= 10.0) {
		}

		if ($i > 10.1) {
		}
		if ($i >= 10.1) {
		}
		if ($i < 10.1) {
		}
		if ($i <= 10.1) {
		}
	}

	public function floatZero(?int $i): void
	{
		if ($i > 0.0) {
		}
		if ($i >= 0.0) {
		}
		if ($i < 0.0) {
		}
		if ($i <= 0.0) {
		}
	}

	public function ranges(int $a, ?int $b): void
	{
		if ($a >= 17 && $a <= 42) {
			if ($b < $a) {
			}
			if ($b <= $a) {
			}
			if ($b > $a) {
			}
			if ($b >= $a) {
			}
		}

		if ($a >= -17 && $a <= 42) {
			if ($b < $a) {
			}
			if ($b <= $a) {
			}
			if ($b > $a) {
			}
			if ($b >= $a) {
			}
		}
	}

}
