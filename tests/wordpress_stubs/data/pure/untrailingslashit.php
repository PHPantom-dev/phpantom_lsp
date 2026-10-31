<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function untrailingslashit;
use function PHPStan\Testing\assertType;

$value = Faker::string();

if (untrailingslashit($value) === 'foo') {
    assertType("'foo'", untrailingslashit($value));
}

if (untrailingslashit($value) !== 'foo') {
    assertType('string', untrailingslashit($value));
}
