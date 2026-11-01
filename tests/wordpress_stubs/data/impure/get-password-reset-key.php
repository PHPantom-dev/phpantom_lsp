<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function get_password_reset_key;
use function PHPStan\Testing\assertType;

$user = Faker::wpUser();

if (is_wp_error(get_password_reset_key($user))) {
    assertType('string|WP_Error', get_password_reset_key($user)); // SKIP: an `@phpstan-impure` call is narrowed by an earlier check of the same call
}

if (! is_wp_error(get_password_reset_key($user))) {
    assertType('string|WP_Error', get_password_reset_key($user)); // SKIP: an `@phpstan-impure` call is narrowed by an earlier check of the same call
}

if (is_string(get_password_reset_key($user))) {
    assertType('string|WP_Error', get_password_reset_key($user)); // SKIP: an `@phpstan-impure` call is narrowed by an earlier check of the same call
}

if (! is_string(get_password_reset_key($user))) {
    assertType('string|WP_Error', get_password_reset_key($user)); // SKIP: an `@phpstan-impure` call is narrowed by an earlier check of the same call
}
