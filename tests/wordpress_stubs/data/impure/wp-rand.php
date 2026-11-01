<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function wp_rand;
use function PHPStan\Testing\assertType;

if (wp_rand(0, 1) === 1) {
    assertType('int', wp_rand(0, 1));
}
