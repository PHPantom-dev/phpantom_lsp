<?php declare(strict_types = 1);

namespace Bug9400;

use RuntimeException;
use function PHPStan\Testing\assertType;

function foo(string $foo): void
{
	if (!ctype_digit($foo) || ($foo = intval($foo)) < 1) {
		throw new RuntimeException();
	}
	assertType('int<1, max>', $foo);
}
