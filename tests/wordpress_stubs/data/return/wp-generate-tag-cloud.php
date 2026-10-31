<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function wp_generate_tag_cloud;
use function PHPStan\Testing\assertType;

$tags = Faker::array(Faker::wpTerm());

// Default $args['format] value.
assertType('string', wp_generate_tag_cloud($tags));
assertType('string', wp_generate_tag_cloud($tags, [])); // SKIP: an array-shape condition matches any array

// Requesting array
assertType('array<int, string>', wp_generate_tag_cloud($tags, ['format' => 'array', 'key' => 'value']));

// Requesting string
assertType('string', wp_generate_tag_cloud($tags, ['format' => 'list', 'key' => 'value'])); // SKIP: an array-shape condition matches any array
assertType('string', wp_generate_tag_cloud($tags, ['format' => 'flat', 'key' => 'value'])); // SKIP: an array-shape condition matches any array

// Unexpected $args['format] value
assertType('string', wp_generate_tag_cloud($tags, ['format' => 'unexpected', 'key' => 'value'])); // SKIP: an array-shape condition matches any array

// Unknown $args['format] value
assertType('array<int, string>|string', wp_generate_tag_cloud($tags, ['format' => Faker::string(), 'key' => 'value'])); // SKIP: an array-shape condition matches any array
