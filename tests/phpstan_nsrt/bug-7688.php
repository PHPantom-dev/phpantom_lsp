<?php declare(strict_types = 1);

namespace Bug7688;

use function PHPStan\Testing\assertType;

/**
 * @template T of bool|null
 * @param T|string $value
 * @return T|non-empty-string
 */
function foo($value)
{
	if (is_string($value)) {
		assertType('string', $value);
		return $value === '' ? 'non-empty' : $value;
	}

	return $value;
}

/**
 * @template T
 * @param T|string $value
 * @return T|non-empty-string
 */
function bar($value)
{
	if (is_string($value)) {
		assertType('string', $value);
		return $value === '' ? 'non-empty' : $value;
	}

	return $value;
}

/**
 * @template T of bool|null
 * @param T|int $value
 * @return T|int<1,max>
 */
function baz($value)
{
	if (is_int($value)) {
		assertType('int', $value);
		return $value < 1 ? 1 : $value;
	}

	return $value;
}
