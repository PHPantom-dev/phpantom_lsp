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
assertType('null', wp_tag_cloud());

// Echo true
assertType('null', wp_tag_cloud(['echo' => true]));
assertType('null', wp_tag_cloud(['format' => 'flat', 'echo' => true]));
assertType('null', wp_tag_cloud(['format' => 'list', 'echo' => true]));
assertType('null', wp_tag_cloud(['format' => 'unexpected', 'echo' => true]));

// Echo true, but format (maybe) array
assertType('array<int, string>|null', wp_tag_cloud(['format' => 'array', 'echo' => true]));
assertType('array<int, string>|null', wp_tag_cloud(['format' => Faker::string(), 'echo' => true]));

// Echo false
assertType('string|null', wp_tag_cloud(['echo' => false]));
assertType('string|null', wp_tag_cloud(['format' => 'flat', 'echo' => false]));
assertType('string|null', wp_tag_cloud(['format' => 'list', 'echo' => false]));
assertType('array<int, string>|null', wp_tag_cloud(['format' => 'array', 'echo' => false]));
assertType('string|null', wp_tag_cloud(['format' => 'unexpected', 'echo' => false]));
assertType('array<int, string>|string|null', wp_tag_cloud(['format' => Faker::string(), 'echo' => false]));

// Echo unknown
assertType('string|null', wp_tag_cloud(['echo' => Faker::bool()]));
assertType('string|null', wp_tag_cloud(['format' => 'flat', 'echo' => Faker::bool()]));
assertType('string|null', wp_tag_cloud(['format' => 'list', 'echo' => Faker::bool()]));
assertType('array<int, string>|null', wp_tag_cloud(['format' => 'array', 'echo' => Faker::bool()]));
assertType('string|null', wp_tag_cloud(['format' => 'unexpected', 'echo' => Faker::bool()]));
assertType('array<int, string>|string|null', wp_tag_cloud(['format' => Faker::string(), 'echo' => Faker::bool()]));
