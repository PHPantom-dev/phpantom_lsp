<?php // lint >= 8.1

declare(strict_types = 1);

namespace Bug11472;

use function PHPStan\Testing\assertType;

/**
 * @phpstan-return ($maybeFoo is 'foo' ? true : false)
 */
function isFoo(mixed $maybeFoo): bool
{
	return $maybeFoo === 'foo';
}

function (): void {
	assertType('true', isFoo('foo'));
};
