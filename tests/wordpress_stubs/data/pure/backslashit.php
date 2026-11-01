<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function backslashit;
use function PHPStan\Testing\assertType;

$value = Faker::string();

if (backslashit($value) === 'foo') {
    assertType("'foo'", backslashit($value));
}

if (backslashit($value) !== 'foo') {
    assertType('string', backslashit($value));
}
