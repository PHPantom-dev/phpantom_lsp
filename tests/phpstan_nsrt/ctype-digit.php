<?php  // lint >= 8.0

declare(strict_types=1);

namespace CtypeDigit;

use function PHPStan\Testing\assertType;

class Foo
{
	public function foo(mixed $foo): void
	{
		ctype_digit($foo);
		assertType('mixed', $foo);

		if (is_string($foo) && ctype_digit($foo)) {
			assertType('numeric-string', $foo);
		} else {
			assertType('mixed', $foo);
		}

		if (is_int($foo) && ctype_digit($foo)) {
			assertType('int<48, 57>|int<256, max>', $foo);
		} else {
			assertType('mixed~(int<48, 57>|int<256, max>)', $foo); // SKIP
		}

		if (ctype_digit($foo)) {
			assertType('int<48, 57>|int<256, max>|numeric-string', $foo);
			return;
		}

		assertType('mixed~(int<48, 57>|int<256, max>)', $foo); // SKIP (not all numeric strings are covered by ctype_digit)
	}
}
