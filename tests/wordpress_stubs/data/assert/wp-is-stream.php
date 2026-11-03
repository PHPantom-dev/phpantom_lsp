<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function wp_is_stream;
use function PHPStan\Testing\assertType;

// if wp_is_stream() returns true, then $path is a non-falsy string
// with no effect on the type of $path if it returns false

$path = Faker::string();
if (wp_is_stream($path)) {
    assertType('non-falsy-string', $path);
}
assertType('string', $path);

$path = Faker::lowercaseString();
if (wp_is_stream($path)) {
    assertType('lowercase-string&non-falsy-string', $path);
}
assertType('lowercase-string', $path);

// if wp_is_stream() returns false, $path is not narrowed
// with no effect on the type of $path if it returns true

$path = Faker::string();
if (! wp_is_stream($path)) {
    assertType('string', $path);
}
assertType('string', $path);

$path = Faker::lowercaseString();
if (! wp_is_stream($path)) {
    assertType('lowercase-string', $path);
}
assertType('lowercase-string', $path);
