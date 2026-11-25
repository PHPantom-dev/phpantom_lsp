<?php // lint >= 8.1

declare(strict_types = 1);

namespace Bug14249;

use function PHPStan\Testing\assertType;

/**
 * @phpstan-assert-if-true positive-int $value
 */
function is_positive_int(mixed $value): bool {
	return is_int($value) && $value > 0;
}

function f(mixed $v): void {
	$f1 = is_positive_int(...);
	$f2 = 'Bug14249\is_positive_int';

	if (is_positive_int($v)) {
		assertType('int<1, max>', $v);
	}

	if ($f1($v)) {
		assertType('int<1, max>', $v); // SKIP
	}

	if ($f2($v)) {
		assertType('int<1, max>', $v); // SKIP
	}
}


/**
 * @template T of bool
 * @param T $if
 * @phpstan-assert (T is true ? true : false) $condition
 */
function assertIfTemplated(mixed $condition, bool $if)
{
}

function doTemplated(): void {
	$f1 = assertIfTemplated(...);
	$f2 = 'Bug14249\assertIfTemplated';

	$v = getMixed();
	assertIfTemplated($v, true);
	assertType('true', $v);

	$v = getMixed();
	$f1($v, true);
	assertType('true', $v); // SKIP

	$v = getMixed();
	$f2($v, true);
	assertType('true', $v); // SKIP

	$v = getMixed();
	assertIfTemplated($v, false);
	assertType('false', $v);

	$v = getMixed();
	$f1($v, false);
	assertType('false', $v); // SKIP

	$v = getMixed();
	$f2($v, false);
	assertType('false', $v); // SKIP
}

function getMixed(): mixed {}

function maybeCallable() {
	$f2 = 'Bug14249\assertIfTemplated';
	if (rand(0,1)) {
		$f2 = 'notCallable';
	}

	$v = getMixed();
	$f2($v, false);
	assertType('mixed', $v);
}
