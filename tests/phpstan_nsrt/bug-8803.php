<?php // lint >= 8.0

namespace Bug8803;

use function PHPStan\Testing\assertType;

class HelloWorld
{
	public function sayHello(): void
	{
		$from = new \DateTimeImmutable('2023-01-30');
		for ($offset = 1; $offset <= 14; $offset++) {
			$value = $from->format('N') + $offset;
			if ($value > 7) {
			}

			$value2 = $offset + $from->format('N');
			$value3 = '1e3' + $offset;
			$value4 = $offset + '1e3';

			assertType("'1'|'2'|'3'|'4'|'5'|'6'|'7'", $from->format('N'));
			assertType('int<1, 14>', $offset);
			assertType('int<2, 21>', $value); // SKIP: arithmetic on a numeric-string literal and a non-literal int widens to `int|float`
			assertType('int<2, 21>', $value2); // SKIP: arithmetic on a numeric-string literal and a non-literal int widens to `int|float`
			assertType('float', $value3); // SKIP: arithmetic on a numeric-string literal and a non-literal int widens to `int|float`
			assertType('float', $value4); // SKIP: arithmetic on a numeric-string literal and a non-literal int widens to `int|float`
		}
	}

	public function testWithMixed(mixed $a, mixed $b): void
	{
		assertType('(array|float|int)', $a + $b);
		assertType('(float|int)', 3 + $b);
		assertType('(float|int)', $a + 3);
	}
}
