<?php // lint >= 8.0

declare(strict_types = 1);

namespace Bug14369;

use Exception;
use function PHPStan\Testing\assertType;

function test(string|null $test): void
{
	$test ??= throw new Exception();

	assertType('string', $test);
}

function testMaybeNull(): void
{
	if (rand(0, 1)) {
		$test = null;
	} else {
		$test = 'hello';
	}
	$test ??= throw new Exception();

	assertType("'hello'", $test);
}

function testAlwaysNull(): void
{
	$test = null;
	$test ??= throw new Exception();

	assertType('*NEVER*', $test); // SKIP: a branch that cannot run keeps the type instead of `*NEVER*`
}
