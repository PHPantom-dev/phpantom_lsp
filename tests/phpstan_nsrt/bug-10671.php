<?php // lint >= 8.0

declare(strict_types=1);

namespace Bug10671;

use Closure;
use function PHPStan\Testing\assertType;

/**
 * @param Closure(mixed...): mixed $closure
 */
function callClosure(Closure $closure): void
{
}

// Arrow function with variadic - the bug
callClosure(
	fn(...$args) => assertType('array<int|string, mixed>', $args)
);

// Anonymous function with variadic - works correctly
callClosure(
	function (...$args) {
		assertType('array<int|string, mixed>', $args);
	}
);

// Arrow function with typed variadic
callClosure(
	fn(int ...$args) => assertType('array<int|string, int>', $args)
);

// Arrow function with variadic and preceding params
/**
 * @param Closure(string, mixed...): mixed $closure
 */
function callClosure2(Closure $closure): void
{
}

callClosure2(
	fn(string $first, ...$rest) => assertType('array<int|string, mixed>', $rest)
);

callClosure2(
	function (string $first, ...$rest) {
		assertType('array<int|string, mixed>', $rest);
	}
);
