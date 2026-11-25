<?php

namespace Bug6329;

use function PHPStan\Testing\assertType;

/**
 * @param mixed $a
 */
function nonEmptyString1($a): void
{
	if (is_string($a) && '' !== $a || null === $a) {
		assertType('non-empty-string|null', $a);
	}

	if ('' !== $a && is_string($a) || null === $a) {
	}

	if (null === $a || is_string($a) && '' !== $a) {
		assertType('non-empty-string|null', $a);
	}

	if (null === $a || '' !== $a && is_string($a)) {
	}
}

/**
 * @param mixed $a
 */
function nonEmptyString2($a): void
{
	if (is_string($a) && strlen($a) > 0 || null === $a) {
		assertType('non-empty-string|null', $a);
	}

	if (null === $a || is_string($a) && strlen($a) > 0) {
		assertType('non-empty-string|null', $a);
	}
}


/**
 * @param mixed $a
 */
function int1($a): void
{
	if (is_int($a) && 0 !== $a || null === $a) {
	}

	if (0 !== $a && is_int($a) || null === $a) {
	}

	if (null === $a || is_int($a) && 0 !== $a) {
	}

	if (null === $a || 0 !== $a && is_int($a)) {
	}
}

/**
 * @param mixed $a
 */
function int2($a): void
{
	if (is_int($a) && $a > 0 || null === $a) {
		assertType('int<1, max>|null', $a);
	}

	if (null === $a || is_int($a) && $a > 0) {
		assertType('int<1, max>|null', $a);
	}
}


/**
 * @param mixed $a
 */
function true($a): void
{
	if (is_bool($a) && false !== $a || null === $a) {
		assertType('true|null', $a);
	}

	if (false !== $a && is_bool($a) || null === $a) {
	}

	if (null === $a || is_bool($a) && false !== $a) {
		assertType('true|null', $a);
	}

	if (null === $a || false !== $a && is_bool($a)) {
	}
}

/**
 * @param mixed $a
 */
function nonEmptyArray1($a): void
{
	if (is_array($a) && [] !== $a || null === $a) {
		assertType('non-empty-array<mixed, mixed>|null', $a);
	}

	if ([] !== $a && is_array($a) || null === $a) {
	}

	if (null === $a || is_array($a) && [] !== $a) {
		assertType('non-empty-array<mixed, mixed>|null', $a);
	}

	if (null === $a || [] !== $a && is_array($a)) {
	}
}

/**
 * @param mixed $a
 */
function nonEmptyArray2($a): void
{
	if (is_array($a) && count($a) > 0 || null === $a) {
		assertType('non-empty-array<mixed, mixed>|null', $a);
	}

	if (null === $a || is_array($a) && count($a) > 0) {
		assertType('non-empty-array<mixed, mixed>|null', $a);
	}
}

/**
 * @param mixed $a
 * @param mixed $b
 * @param mixed $c
 */
function inverse($a, $b, $c): void
{
	if ((!is_string($a) || '' === $a) && null !== $a) {
	} else {
		assertType('non-empty-string|null', $a);
	}

	if ((!is_int($b) || $b <= 0) && null !== $b) {
	} else {
		assertType('int<1, max>|null', $b);
	}

	if (null !== $c && (!is_array($c) || count($c) <= 0)) {
	} else {
		assertType('non-empty-array<mixed, mixed>|null', $c);
	}
}

/**
 * @param mixed $a
 * @param mixed $b
 * @param mixed $c
 * @param mixed $d
 */
function combinations($a, $b, $c, $d): void
{
	if (is_string($a) && '' !== $a || is_int($a) && $a > 0 || null === $a) {
		assertType('int<1, max>|non-empty-string|null', $a);
	}
	if ((!is_string($b) || '' === $b) && (!is_int($b) || $b <= 0) && null !== $b) {
	} else {
		assertType('int<1, max>|non-empty-string|null', $b);
	}

	if (is_array($c) && $c === array_filter($c, 'is_string', ARRAY_FILTER_USE_KEY) || null === $c) {
		assertType('array<string, mixed>|null', $c); // SKIP: comparing an array with `array_filter()` of itself narrows nothing
	}
	if ((!is_array($d) || $d !== array_filter($d, 'is_string', ARRAY_FILTER_USE_KEY)) && null !== $d) {
	} else {
		assertType('array<string, mixed>|null', $d); // SKIP: comparing an array with `array_filter()` of itself narrows nothing
	}
}
