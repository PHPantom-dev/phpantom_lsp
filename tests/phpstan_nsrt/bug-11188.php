<?php // lint >= 8.0

namespace Bug11188;

use DateTime;
use function PHPStan\Testing\assertType;

/**
 * @template TDefault of string
 * @template TExplicit of string
 *
 * @param TDefault $abstract
 * @param array<string, mixed> $parameters
 * @param TExplicit|null $type
 * @return (
 *   $type is class-string ? new<TExplicit> :
 *   $abstract is class-string ? new<TDefault> : mixed
 * )
 */
function instance(string $abstract, array $parameters = [], ?string $type = null): mixed
{
	return 'something';
}

function (): void {
	assertType(DateTime::class, instance('cache', [], DateTime::class));
	assertType(DateTime::class, instance(DateTime::class));
};
