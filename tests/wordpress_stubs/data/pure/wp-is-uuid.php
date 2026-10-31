<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function wp_is_uuid;
use function PHPStan\Testing\assertType;

$uuid = Faker::string();

assertType('bool', wp_is_uuid($uuid));

if (wp_is_uuid($uuid) === true) {
    assertType('true', wp_is_uuid($uuid));
}

if (wp_is_uuid($uuid) === false) {
    assertType('false', wp_is_uuid($uuid));
}

if (wp_is_uuid($uuid) !== true) {
    assertType('false', wp_is_uuid($uuid));
}
