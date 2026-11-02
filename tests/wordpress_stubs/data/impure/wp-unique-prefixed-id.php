<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function wp_unique_prefixed_id;
use function PHPStan\Testing\assertType;

if (wp_unique_prefixed_id() === 'prefix1') {
    assertType('lowercase-string&non-falsy-string&numeric-string', wp_unique_prefixed_id());
}
