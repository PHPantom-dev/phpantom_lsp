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
assertType('null', wp_get_archives());

// Explicit value of true|1
assertType('null', wp_get_archives(['echo' => true, 'key' => 'value']));
assertType('null', wp_get_archives(['echo' => 1, 'key' => 'value']));

// Explicit value of false|0
assertType('string|null', wp_get_archives(['echo' => false, 'key' => 'value']));
assertType('string|null', wp_get_archives(['echo' => 0, 'key' => 'value']));

// Unknown value
assertType('string|null', wp_get_archives(['echo' => Faker::bool(), 'key' => 'value']));
assertType('string|null', wp_get_archives(['echo' => Faker::int(), 'key' => 'value']));
