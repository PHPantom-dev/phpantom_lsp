<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function rest_sanitize_boolean;
use function PHPStan\Testing\assertType;

$toSanitize = Faker::mixed();

assertType('bool', rest_sanitize_boolean($toSanitize));

if (rest_sanitize_boolean($toSanitize)) {
    assertType('true', rest_sanitize_boolean($toSanitize));
}

if (! rest_sanitize_boolean($toSanitize)) {
    assertType('false', rest_sanitize_boolean($toSanitize));
}
