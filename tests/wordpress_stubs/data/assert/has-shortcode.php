<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function has_shortcode;
use function PHPStan\Testing\assertType;

$content = Faker::string();
$tag = Faker::string();
if (has_shortcode($content, $tag)) {
    assertType('non-falsy-string', $content);
    assertType('non-empty-string', $tag);
}
assertType('string', $content);
assertType('string', $tag);

// Check that types are not generalized
$content = 'content';
$tag = 'tag';
if (has_shortcode($content, $tag)) {
    assertType("'content'", $content);
    assertType("'tag'", $tag);
}
