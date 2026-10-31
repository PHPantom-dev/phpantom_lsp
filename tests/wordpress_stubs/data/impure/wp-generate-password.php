<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function wp_generate_password;
use function PHPStan\Testing\assertType;

if (wp_generate_password() === 'password') {
    assertType('string', wp_generate_password());
}

if (wp_generate_password() !== 'password') {
    assertType('string', wp_generate_password());
}
