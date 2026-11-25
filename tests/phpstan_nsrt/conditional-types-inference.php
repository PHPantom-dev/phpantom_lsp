<?php // lint >= 8.0

namespace ConditionalTypesInference;


use function PHPStan\Testing\assertType;

/**
 * @return ($value is int ? true : false)
 */
function testIsInt(mixed $value): bool
{
	return is_int($value);
}

/**
 * @return ($value is not int ? true : false)
 */
function testIsNotInt(mixed $value): bool
{
	return !is_int($value);
}

/**
 * @return ($value is int ? void : never)
 */
function assertIsInt(mixed $value): void {
	assert(is_int($value));
}

function (mixed $value) {
	if (testIsInt($value)) {
		assertType('int', $value);
	} else {
	}

	if (testIsNotInt($value)) {
	} else {
		assertType('int', $value);
	}

	assertIsInt($value);
	assertType('int', $value);
};

function (string $value) {
	if (testIsInt($value)) {
		assertType('*NEVER*', $value); // SKIP: a branch that cannot run keeps the type instead of `*NEVER*`
	} else {
		assertType('string', $value);
	}

	if (testIsNotInt($value)) {
		assertType('string', $value);
	} else {
		assertType('*NEVER*', $value); // SKIP: a branch that cannot run keeps the type instead of `*NEVER*`
	}

	assertIsInt($value);
	assertType('*NEVER*', $value); // SKIP: a branch that cannot run keeps the type instead of `*NEVER*`
};

function (int $value) {
	if (testIsInt($value)) {
		assertType('int', $value);
	} else {
		assertType('*NEVER*', $value);
	}

	if (testIsNotInt($value)) {
		assertType('*NEVER*', $value);
	} else {
		assertType('int', $value);
	}

	assertIsInt($value);
	assertType('int', $value);
};

/**
 * @return ($condition is true ? void : never)
 */
function invariant(bool $condition, string $message): void
{
	assert($condition, $message);
}

function (mixed $value) {
	invariant(is_array($value), 'must be array');
	assertType('array<mixed, mixed>', $value);
};
