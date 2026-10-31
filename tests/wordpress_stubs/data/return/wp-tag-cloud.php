<?php

/**
 * Note:
 * Starting from PHPStan 1.10.49, void types, including void in unions, are
 * transformed into null.
 *
 * @link https://github.com/phpstan/phpstan-src/pull/2778
 */

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function PHPStan\Testing\assertType;
use function wp_tag_cloud;

// Default value
assertType('null', wp_tag_cloud()); // SKIP: `void` from a conditional return type is not read as `null`

// Echo true
assertType('null', wp_tag_cloud(['echo' => true])); // SKIP: an array-shape condition matches any array
assertType('null', wp_tag_cloud(['format' => 'flat', 'echo' => true])); // SKIP: an array-shape condition matches any array
assertType('null', wp_tag_cloud(['format' => 'list', 'echo' => true])); // SKIP: an array-shape condition matches any array
assertType('null', wp_tag_cloud(['format' => 'unexpected', 'echo' => true])); // SKIP: an array-shape condition matches any array

// Echo true, but format (maybe) array
assertType('array<int, string>|null', wp_tag_cloud(['format' => 'array', 'echo' => true])); // SKIP: `void` from a conditional return type is not read as `null`
assertType('array<int, string>|null', wp_tag_cloud(['format' => Faker::string(), 'echo' => true])); // SKIP: `void` from a conditional return type is not read as `null`

// Echo false
assertType('string|null', wp_tag_cloud(['echo' => false])); // SKIP: an array-shape condition matches any array
assertType('string|null', wp_tag_cloud(['format' => 'flat', 'echo' => false])); // SKIP: an array-shape condition matches any array
assertType('string|null', wp_tag_cloud(['format' => 'list', 'echo' => false])); // SKIP: an array-shape condition matches any array
assertType('array<int, string>|null', wp_tag_cloud(['format' => 'array', 'echo' => false])); // SKIP: `void` from a conditional return type is not read as `null`
assertType('string|null', wp_tag_cloud(['format' => 'unexpected', 'echo' => false])); // SKIP: an array-shape condition matches any array
assertType('array<int, string>|string|null', wp_tag_cloud(['format' => Faker::string(), 'echo' => false])); // SKIP: an array-shape condition matches any array

// Echo unknown
assertType('string|null', wp_tag_cloud(['echo' => Faker::bool()])); // SKIP: an array-shape condition matches any array
assertType('string|null', wp_tag_cloud(['format' => 'flat', 'echo' => Faker::bool()])); // SKIP: an array-shape condition matches any array
assertType('string|null', wp_tag_cloud(['format' => 'list', 'echo' => Faker::bool()])); // SKIP: an array-shape condition matches any array
assertType('array<int, string>|null', wp_tag_cloud(['format' => 'array', 'echo' => Faker::bool()])); // SKIP: `void` from a conditional return type is not read as `null`
assertType('string|null', wp_tag_cloud(['format' => 'unexpected', 'echo' => Faker::bool()])); // SKIP: an array-shape condition matches any array
assertType('array<int, string>|string|null', wp_tag_cloud(['format' => Faker::string(), 'echo' => Faker::bool()])); // SKIP: an array-shape condition matches any array
