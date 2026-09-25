<?php

namespace CountMaybe;

use Countable;
use function PHPStan\Testing\assertType;

function doBar1(float $notCountable, int $mode): void
{
	if (count($notCountable, $mode) > 0) {
		assertType('float', $notCountable);
	} else {
		assertType('float', $notCountable);
	}
	assertType('float', $notCountable);
}

/**
 * @param array|int $maybeMode
 */
function doBar2(float $notCountable, $maybeMode): void
{
	if (count($notCountable, $maybeMode) > 0) {
		assertType('float', $notCountable);
	} else {
		assertType('float', $notCountable);
	}
	assertType('float', $notCountable);
}

function doBar3(float $notCountable, float $invalidMode): void
{
	if (count($notCountable, $invalidMode) > 0) {
		assertType('float', $notCountable);
	} else {
		assertType('float', $notCountable);
	}
	assertType('float', $notCountable);
}

/**
 * @param float|int[] $maybeCountable
 */
function doFoo1($maybeCountable, int $mode): void
{
	if (count($maybeCountable, $mode) > 0) {
	} else {
		// PHPantom is more precise than PHPStan here: count($x, $mode) > 0 being false means count is 0, so the array part is empty whatever the mode (PHP 8 throws ValueError for an invalid mode). PHPStan just skips narrowing when a mode argument is passed.
		assertType('float|array{}', $maybeCountable);
	}
	assertType('array<int>|float', $maybeCountable);
}

/**
 * @param float|int[] $maybeCountable
 * @param array|int $maybeMode
 */
function doFoo2($maybeCountable, $maybeMode): void
{
	if (count($maybeCountable, $maybeMode) > 0) {
	} else {
		// PHPantom is more precise than PHPStan here: same as line 49: count 0 means empty array for any valid mode.
		assertType('float|array{}', $maybeCountable);
	}
	assertType('array<int>|float', $maybeCountable);
}

/**
 * @param float|int[] $maybeCountable
 */
function doFoo3($maybeCountable, float $invalidMode): void
{
	if (count($maybeCountable, $invalidMode) > 0) {
	} else {
		// PHPantom is more precise than PHPStan here: same as line 49.
		assertType('float|array{}', $maybeCountable);
	}
	assertType('array<int>|float', $maybeCountable);
}

/**
 * @param float|list<int> $maybeCountable
 */
function doFoo4($maybeCountable, int $mode): void
{
	if (count($maybeCountable, $mode) > 0) {
	} else {
		// PHPantom is more precise than PHPStan here: same as line 49: an empty list is array{}.
		assertType('float|array{}', $maybeCountable);
	}
	assertType('float|list<int>', $maybeCountable);
}

/**
 * @param float|list<int> $maybeCountable
 * @param array|int $maybeMode
 */
function doFoo5($maybeCountable, $maybeMode): void
{
	if (count($maybeCountable, $maybeMode) > 0) {
	} else {
		// PHPantom is more precise than PHPStan here: same as line 49.
		assertType('float|array{}', $maybeCountable);
	}
	assertType('float|list<int>', $maybeCountable);
}

/**
 * @param float|list<int> $maybeCountable
 */
function doFoo6($maybeCountable, float $invalidMode): void
{
	if (count($maybeCountable, $invalidMode) > 0) {
	} else {
		// PHPantom is more precise than PHPStan here: same as line 49.
		assertType('float|array{}', $maybeCountable);
	}
	assertType('float|list<int>', $maybeCountable);
}

/**
 * @param float|list<int>|Countable $maybeCountable
 */
function doFoo7($maybeCountable, int $mode): void
{
	if (count($maybeCountable, $mode) > 0) {
	} else {
		// PHPantom is more precise than PHPStan here: same as line 49. The Countable part is kept correctly.
		assertType('float|array{}|Countable', $maybeCountable);
	}
	assertType('Countable|float|list<int>', $maybeCountable);
}

/**
 * @param float|list<int>|Countable $maybeCountable
 * @param array|int $maybeMode
 */
function doFoo8($maybeCountable, $maybeMode): void
{
	if (count($maybeCountable, $maybeMode) > 0) {
	} else {
		// PHPantom is more precise than PHPStan here: same as line 129.
		assertType('float|array{}|Countable', $maybeCountable);
	}
	assertType('Countable|float|list<int>', $maybeCountable);
}

/**
 * @param float|list<int>|Countable $maybeCountable
 */
function doFoo9($maybeCountable, float $invalidMode): void
{
	if (count($maybeCountable, $invalidMode) > 0) {
	} else {
		// PHPantom is more precise than PHPStan here: same as line 129.
		assertType('float|array{}|Countable', $maybeCountable);
	}
	assertType('Countable|float|list<int>', $maybeCountable);
}

function doFooBar1(array $countable, int $mode): void
{
	if (count($countable, $mode) > 0) {
		assertType('non-empty-array', $countable);
	} else {
		assertType('array{}', $countable);
	}
	assertType('array', $countable);
}

/**
 * @param array|int $maybeMode
 */
function doFooBar2(array $countable, $maybeMode): void
{
	if (count($countable, $maybeMode) > 0) {
		assertType('non-empty-array', $countable);
	} else {
		assertType('array{}', $countable);
	}
	assertType('array', $countable);
}

function doFooBar3(array $countable, float $invalidMode): void
{
	if (count($countable, $invalidMode) > 0) {
		assertType('non-empty-array', $countable);
	} else {
		assertType('array{}', $countable);
	}
	assertType('array', $countable);
}
