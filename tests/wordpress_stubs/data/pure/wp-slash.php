<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function wp_slash;
use function PHPStan\Testing\assertType;

$value = Faker::string();
if (wp_slash($value) === 'foo') {
    assertType("'foo'", wp_slash($value));
}

$value = Faker::array();
if (wp_slash($value) === ['foo' => 'bar']) {
    assertType("array{foo: 'bar'}", wp_slash($value));
}
