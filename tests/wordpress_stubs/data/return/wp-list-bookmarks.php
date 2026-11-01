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

use function wp_list_bookmarks;
use function PHPStan\Testing\assertType;

// Default value of true
assertType('null', wp_list_bookmarks());

// Explicit value of true|1
assertType('null', wp_list_bookmarks(['echo' => true, 'key' => 'value']));
assertType('null', wp_list_bookmarks(['echo' => 1, 'key' => 'value']));

// Explicit value of false|0
assertType('string', wp_list_bookmarks(['echo' => false, 'key' => 'value']));
assertType('string', wp_list_bookmarks(['echo' => 0, 'key' => 'value']));

// Unknown value
assertType('string|null', wp_list_bookmarks(['echo' => Faker::bool(), 'key' => 'value']));
assertType('string|null', wp_list_bookmarks(['echo' => Faker::int(), 'key' => 'value']));
