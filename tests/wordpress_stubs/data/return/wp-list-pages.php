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

use function wp_list_pages;
use function PHPStan\Testing\assertType;

// Default value of true
assertType('null', wp_list_pages());

// Explicit value of true
assertType('null', wp_list_pages(['echo' => true, 'key' => 'value']));

// Explicit value of false
assertType('string', wp_list_pages(['echo' => false, 'key' => 'value']));

// Unknown value
assertType('string|null', wp_list_pages(['echo' => Faker::bool(), 'key' => 'value']));
