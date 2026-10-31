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

use function wp_get_archives;
use function PHPStan\Testing\assertType;

// Default value of true
assertType('null', wp_get_archives()); // SKIP: `void` from a conditional return type is not read as `null`

// Explicit value of true|1
assertType('null', wp_get_archives(['echo' => true, 'key' => 'value'])); // SKIP: an array-shape condition matches any array
assertType('null', wp_get_archives(['echo' => 1, 'key' => 'value'])); // SKIP: an array-shape condition matches any array

// Explicit value of false|0
assertType('string|null', wp_get_archives(['echo' => false, 'key' => 'value'])); // SKIP: `void` from a conditional return type is not read as `null`
assertType('string|null', wp_get_archives(['echo' => 0, 'key' => 'value'])); // SKIP: `void` from a conditional return type is not read as `null`

// Unknown value
assertType('string|null', wp_get_archives(['echo' => Faker::bool(), 'key' => 'value'])); // SKIP: `void` from a conditional return type is not read as `null`
assertType('string|null', wp_get_archives(['echo' => Faker::int(), 'key' => 'value'])); // SKIP: `void` from a conditional return type is not read as `null`
