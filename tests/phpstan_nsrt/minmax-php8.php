<?php // lint >= 8.0

namespace MinMaxArraysPhp8;

use function PHPStan\Testing\assertType;

function dummy(): void
{
	assertType('1', min([1]));
	assertType('*ERROR*', min([]));
	assertType('1', max([1]));
	assertType('*ERROR*', max([]));
}

/**
 * @param int[] $ints
 */
function dummy2(array $ints): void
{
	if (count($ints) === 0) {
		assertType('*ERROR*', min($ints));
		assertType('*ERROR*', max($ints));
	} else {
		assertType('int', min($ints));
		assertType('int', max($ints));
	}
	if (count($ints) === 1) {
		assertType('int', min($ints));
		assertType('int', max($ints));
	} else {
		assertType('int', min($ints));
		assertType('int', max($ints));
	}
	if (count($ints) !== 0) {
		assertType('int', min($ints));
		assertType('int', max($ints));
	} else {
		assertType('*ERROR*', min($ints));
		assertType('*ERROR*', max($ints));
	}
	if (count($ints) !== 1) {
		assertType('int', min($ints));
		assertType('int', max($ints));
	} else {
		assertType('int', min($ints));
		assertType('int', max($ints));
	}
	if (count($ints) > 0) {
		assertType('int', min($ints));
		assertType('int', max($ints));
	} else {
		assertType('*ERROR*', min($ints));
		assertType('*ERROR*', max($ints));
	}
	if (count($ints) >= 1) {
		assertType('int', min($ints));
		assertType('int', max($ints));
	} else {
		assertType('*ERROR*', min($ints));
		assertType('*ERROR*', max($ints));
	}
	if (count($ints) >= 2) {
		assertType('int', min($ints));
		assertType('int', max($ints));
	} else {
		assertType('int', min($ints));
		assertType('int', max($ints));
	}
	if (count($ints) <= 0) {
		assertType('*ERROR*', min($ints));
		assertType('*ERROR*', max($ints));
	} else {
		assertType('int', min($ints));
		assertType('int', max($ints));
	}
	if (count($ints) < 1) {
		assertType('*ERROR*', min($ints));
		assertType('*ERROR*', max($ints));
	} else {
		assertType('int', min($ints));
		assertType('int', max($ints));
	}
	if (count($ints) < 2) {
		assertType('int', min($ints));
		assertType('int', max($ints));
	} else {
		assertType('int', min($ints));
		assertType('int', max($ints));
	}
}

/**
 * @param int[] $ints
 */
function dummy3(array $ints): void
{
	assertType('int', min($ints));
	assertType('int', max($ints));
}


function dummy4(\DateTimeInterface $dateA, ?\DateTimeInterface $dateB): void
{
	assertType('array{0: DateTimeInterface, 1?: DateTimeInterface}', array_filter([$dateA, $dateB]));
	assertType('DateTimeInterface', min(array_filter([$dateA, $dateB])));
	assertType('DateTimeInterface', max(array_filter([$dateA, $dateB])));
	assertType('array{0?: DateTimeInterface}', array_filter([$dateB]));
	assertType('DateTimeInterface', min(array_filter([$dateB])));
	assertType('DateTimeInterface', max(array_filter([$dateB])));
}


class HelloWorld
{
	public function unionType(): void
	{
		/**
		 * @var array<0|1|2|3|4|5|6|7|8|9>
		 */
		$numbers = getFoo();

		assertType('0|1|2|3|4|5|6|7|8|9', min($numbers));
		assertType('0', min([0, 1, 2, 3, 4, 5, 6, 7, 8, 9]));

		assertType('0|1|2|3|4|5|6|7|8|9', max($numbers));
		assertType('9', max([0, 1, 2, 3, 4, 5, 6, 7, 8, 9]));
	}
}

/**
 * @param int[] $ints
 */
function countMode(array $ints, int $mode): void
{
	if (count($ints, $mode) > 0) {
		assertType('int', min($ints));
		assertType('int', max($ints));
	} else {
		assertType('*ERROR*', min($ints));
		assertType('*ERROR*', max($ints));
	}
}

/**
 * @param int[] $ints
 */
function countNormal(array $ints): void
{
	if (count($ints, COUNT_NORMAL) > 0) {
		assertType('int', min($ints));
		assertType('int', max($ints));
	} else {
		assertType('*ERROR*', min($ints));
		assertType('*ERROR*', max($ints));
	}
}

/**
 * @param int[] $ints
 */
function countRecursive(array $ints): void
{
	if (count($ints, COUNT_RECURSIVE) < 1) {
		assertType('*ERROR*', min($ints));
		assertType('*ERROR*', max($ints));
	} else {
		assertType('int', min($ints));
		assertType('int', max($ints));
	}
	if (count($ints, COUNT_RECURSIVE) < 2) {
		assertType('int', min($ints));
		assertType('int', max($ints));
	} else {
		assertType('int', min($ints));
		assertType('int', max($ints));
	}
}
