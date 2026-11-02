<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function trailingslashit;
use function PHPStan\Testing\assertType;

$toSlash = Faker::string();

if (trailingslashit($toSlash) === 'foo') {
    assertType("'foo'", trailingslashit($toSlash));
}

if (trailingslashit($toSlash) !== 'foo') {
    assertType('non-falsy-string', trailingslashit($toSlash));
}
