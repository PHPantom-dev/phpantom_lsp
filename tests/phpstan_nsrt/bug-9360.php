<?php declare(strict_types = 1);

namespace Bug9360;

use function PHPStan\Testing\assertType;

/**
 * @template T of string|bool
 *
 * @param T ...$args
 *
 * @return (T is string ? string : bool)
 */
function environment(string|bool ...$args): string|bool
{
	if (count($args) === 0) {
		return true;
	}
	return (string) $args[0];
}

assertType('string|bool', environment()); // PHPStan says `string`, but with no arguments the function returns `true`
assertType('string', environment('APP_ENV'));
assertType('string', environment('APP_ENV', 'production'));
