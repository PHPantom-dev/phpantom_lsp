<?php declare(strict_types=1);

namespace Bug7483;

use function PHPStan\Testing\assertType;

class A {}

/**
 * @param class-string<A> $class
 */
function bar($class): string
{
	assertType('class-string<Bug7483\\A>', ltrim($class, '\\')); // PHPantom keeps the class
}

/**
 * @param class-string $class
 * @return class-string
 */
function foo($class): string
{
	assertType('class-string', ltrim($class, '\\'));
	assertType("'Bug7483\\\\A'", ltrim(A::class, '\\'));
}
