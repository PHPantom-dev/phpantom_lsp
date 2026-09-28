<?php declare(strict_types = 1);

namespace Bug7805;

use function PHPStan\Testing\assertNativeType;
use function PHPStan\Testing\assertType;

/**
 * @phpstan-param array{help?: null} $params
 */
function foo(array $params)
{
	assertType('array{help?: null}', $params);
	assertNativeType('array', $params);
	if (array_key_exists('help', $params)) {
		assertType('array{help: null}', $params);
		assertNativeType("non-empty-array&hasOffset('help')", $params);
		unset($params['help']);

		assertType('array{}', $params);
		assertNativeType("array<mixed~'help', mixed>", $params);
		$params = $params === [] ? ['list'] : $params;
		assertType("array{'list'}", $params);
		assertNativeType("array{'list'}", $params);
		array_unshift($params, 'help');
		assertType("array{'help', 'list'}", $params);
		assertNativeType("array{'help', 'list'}", $params);
	}
	assertType("array{}|array{'help', 'list'}", $params);
	assertNativeType('array', $params);

	return $params;
}
