<?php

namespace Bug12731;

use function PHPStan\Testing\assertType;

/** @phpstan-impure */
function impure_int(): int {
	return rand(0, 100) - 50;
}

/** @phpstan-pure */
function pure_int(): int {
	return -42;
}

assertType('int<4, max>', max(4, pure_int()));

$_ = impure_int();
assertType('int<4, max>', max(4, $_));

assertType('int<4, max>', max(4, impure_int()));
assertType('int<4, max>', max(impure_int(), 4));
assertType('int', impure_int());
