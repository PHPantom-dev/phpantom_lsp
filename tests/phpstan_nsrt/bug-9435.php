<?php // lint >= 8.0

declare(strict_types=1);

namespace Bug9435;

use function PHPStan\Testing\assertType;

function x(): mixed
{
	return null;
}

/** @phpstan-assert ($allow_null is true ? string|null : string) $input */
function trueCheck(mixed $input, bool $allow_null = false): void
{
}

$a = x();
trueCheck($a);
assertType('string', $a); // incorrect: should be string but is string|null

$a = x();
trueCheck($a, false);
assertType('string', $a); // correct (string)

$a = x();
trueCheck($a, allow_null: false);
assertType('string', $a); // correct (string)

$a = x();
trueCheck(allow_null: false, input: $a);
assertType('string', $a); // correct (string)

$a = x();
trueCheck($a, true);
assertType('string|null', $a); // correct (string|null)

$a = x();
trueCheck($a, allow_null: true);
assertType('string|null', $a); // correct (string|null)

$a = x();
trueCheck(allow_null: true, input: $a);
assertType('string|null', $a); // correct (string|null)

/** @phpstan-assert ($allow_null is false ? string : string|null) $input */
function falseCheck(mixed $input, bool $allow_null = false): void
{
}

$a = x();
falseCheck($a);
assertType('string', $a); // incorrect: should be string but is string|null

$a = x();
falseCheck($a, false);
assertType('string', $a); // correct (string)

$a = x();
falseCheck($a, allow_null: false);
assertType('string', $a); // correct (string)

$a = x();
falseCheck(allow_null: false, input: $a);
assertType('string', $a); // correct (string|null)

$a = x();
falseCheck($a, true);
assertType('string|null', $a); // correct (string|null)

$a = x();
falseCheck($a, allow_null: true);
assertType('string|null', $a); // correct (string|null)

$a = x();
falseCheck(allow_null: true, input: $a);
assertType('string|null', $a); // correct (string|null)

/** @phpstan-assert ($allow_null is not true ? string : string|null) $input */
function notTrueCheck(mixed $input, bool $allow_null = false): void
{
}

$a = x();
notTrueCheck($a);
assertType('string', $a); // incorrect: should be string but is string|null

$a = x();
notTrueCheck($a, false);
assertType('string', $a); // correct (string)

$a = x();
notTrueCheck($a, allow_null: false);
assertType('string', $a); // correct (string)

$a = x();
notTrueCheck(allow_null: false, input: $a);
assertType('string', $a); // correct (string|null)

$a = x();
notTrueCheck($a, true);
assertType('string|null', $a); // correct (string|null)

$a = x();
notTrueCheck($a, allow_null: true);
assertType('string|null', $a); // correct (string|null)

$a = x();
notTrueCheck(allow_null: true, input: $a);
assertType('string|null', $a); // correct (string|null)

/** @phpstan-assert ($allow_null is not false ? string|null : string) $input */
function notFalseCheck(mixed $input, bool $allow_null = false): void
{
}

$a = x();
notFalseCheck($a);
assertType('string', $a); // incorrect: should be string but is string|null

$a = x();
notFalseCheck($a, false);
assertType('string', $a); // correct (string)

$a = x();
notFalseCheck($a, allow_null: false);
assertType('string', $a); // correct (string)

$a = x();
notFalseCheck(allow_null: false, input: $a);
assertType('string', $a); // correct (string|null)

$a = x();
notFalseCheck($a, true);
assertType('string|null', $a); // correct (string|null)

$a = x();
notFalseCheck($a, allow_null: true);
assertType('string|null', $a); // correct (string|null)

$a = x();
notFalseCheck(allow_null: true, input: $a);
assertType('string|null', $a); // correct (string|null)

/** @phpstan-assert ($allow_null is false ? string : string|null) $input */
function checkWithVariadics(mixed $input, bool $allow_null = false, ...$more): void
{
}

$a = x();
checkWithVariadics($a);
assertType('string', $a); // incorrect: should be string but is string|null

$a = x();
checkWithVariadics($a, false);
assertType('string', $a); // correct (string)

$a = x();
checkWithVariadics($a, allow_null: false);
assertType('string', $a); // correct (string)

$a = x();
checkWithVariadics(allow_null: false, input: $a);
assertType('string', $a); // correct (string)

$a = x();
checkWithVariadics($a, true);
assertType('string|null', $a); // correct (string|null)

$a = x();
checkWithVariadics($a, allow_null: true);
assertType('string|null', $a); // correct (string|null)

$a = x();
checkWithVariadics(allow_null: true, input: $a);
assertType('string|null', $a); // correct (string|null)
