<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function shortcode_exists;
use function PHPStan\Testing\assertType;

$tag = Faker::string();
if (shortcode_exists($tag)) {
    assertType('non-empty-string', $tag);
}
if (! shortcode_exists($tag)) {
    assertType('string', $tag);
}

// Check that constant strings are not generalized
$tag = 'tag';
if (shortcode_exists($tag)) {
    assertType("'tag'", $tag);
}
