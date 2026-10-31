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

use function wp_list_categories;
use function PHPStan\Testing\assertType;

// Default value of 1
assertType('false|null', wp_list_categories()); // SKIP: `void` from a conditional return type is not read as `null`

// Explicit value of true|1
assertType('false|null', wp_list_categories(['echo' => true, 'key' => 'value'])); // SKIP: an array-shape condition matches any array
assertType('false|null', wp_list_categories(['echo' => 1, 'key' => 'value'])); // SKIP: an array-shape condition matches any array

// Explicit value of false|0
assertType('string|false', wp_list_categories(['echo' => false, 'key' => 'value']));
assertType('string|false', wp_list_categories(['echo' => 0, 'key' => 'value']));

// Unknown value
assertType('string|false|null', wp_list_categories(['echo' => Faker::bool(), 'key' => 'value'])); // SKIP: an array-shape condition matches any array
assertType('string|false|null', wp_list_categories(['echo' => Faker::int(), 'key' => 'value'])); // SKIP: an array-shape condition matches any array
