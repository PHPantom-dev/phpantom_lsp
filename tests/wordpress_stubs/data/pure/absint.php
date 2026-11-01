<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function absint;
use function PHPStan\Testing\assertType;

$intArg = Faker::int();

assertType('int<0, max>', absint($intArg));

if (absint($intArg) === 1) {
    assertType('1', absint($intArg));
}

if (absint($intArg) > 1) {
    assertType('int<2, max>', absint($intArg));
}
