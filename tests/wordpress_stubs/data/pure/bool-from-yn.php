<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function bool_from_yn;
use function PHPStan\Testing\assertType;

$strArg = Faker::string();

assertType('bool', bool_from_yn($strArg));

if (bool_from_yn($strArg) === true) {
    assertType('true', bool_from_yn($strArg));
}

if (bool_from_yn($strArg) === false) {
    assertType('false', bool_from_yn($strArg));
}

if (bool_from_yn($strArg) !== true) {
    assertType('false', bool_from_yn($strArg));
}
