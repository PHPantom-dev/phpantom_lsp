<?php
namespace Bug13546;

use function PHPStan\Testing\assertType;

/** @param array<int> $array */
function first(array $array): void
{
	if (array_key_first($array) !== null) {
	} else {
		assertType('array{}', $array); // SKIP: a null check on `array_key_first()`/`array_key_last()` does not narrow the array
	}
	assertType('array<int>', $array);
}

/** @param array<int> $array */
function firstReversed(array $array): void
{
	if (null !== array_key_first($array)) {
	} else {
		assertType('array{}', $array); // SKIP: a null check on `array_key_first()`/`array_key_last()` does not narrow the array
	}
	assertType('array<int>', $array);
}

/** @param array<int> $array */
function last(array $array): void
{
	if (array_key_last($array) !== null) {
	} else {
		assertType('array{}', $array); // SKIP: a null check on `array_key_first()`/`array_key_last()` does not narrow the array
	}
	assertType('array<int>', $array);
}

function maybeArray(array $array, $mixed): void
{
	$arrayOrMixed = rand(0, 1) ? $array : $mixed;

	if (array_key_last($arrayOrMixed) !== null) {
		assertType('mixed', $arrayOrMixed);
	} else {
		assertType('mixed', $arrayOrMixed);
	}
	assertType('mixed', $arrayOrMixed);
}

function mixedLast($mixed): void
{
	if (is_array($mixed)) {
		return;
	}

	if (array_key_last($mixed) !== null) {
	} else {
	}
}

/** @param list<string> $array */
function firstInCondition(array $array)
{
	if (($key = array_key_first($array)) !== null) {
		return $array[$key];
	}
	assertType('array{}', $array); // SKIP: a null check on `array_key_first()`/`array_key_last()` does not narrow the array
	return null;
}

/** @param list<string> $array */
function maybeNull(array $array, ?int $nullOrInt, ?string $nullOrString): void
{
	if (array_key_first($array) !== $nullOrInt) {
		assertType('list<string>', $array);
	}
	if (array_key_first($array) !== $nullOrString) {
		assertType('list<string>', $array);
	}
	assertType('list<string>', $array);

	if (array_key_last($array) !== $nullOrInt) {
		assertType('list<string>', $array);
	}
	if (array_key_last($array) !== $nullOrString) {
		assertType('list<string>', $array);
	}
	assertType('list<string>', $array);
}
