<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function wp_nonce_tick;
use function PHPStan\Testing\assertType;

if (wp_nonce_tick('action') === 1.23) {
    assertType('float', wp_nonce_tick('action'));
}

if (wp_nonce_tick('action') === Faker::mixed()) {
    assertType('float', wp_nonce_tick('action'));
}
