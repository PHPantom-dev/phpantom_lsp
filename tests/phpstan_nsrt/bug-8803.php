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
			assertType('int<2, 21>', $value);
			assertType('int<2, 21>', $value2);
			assertType('float', $value3);
			assertType('float', $value4);
		}
	}

	public function testWithMixed(mixed $a, mixed $b): void
	{
		assertType('(array|float|int)', $a + $b);
		assertType('(float|int)', 3 + $b);
		assertType('(float|int)', $a + 3);
	}
}
