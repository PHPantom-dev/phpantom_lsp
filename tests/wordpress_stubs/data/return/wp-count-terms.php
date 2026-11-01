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

use function wp_count_terms;
use function PHPStan\Testing\assertType;

assertType('0|numeric-string|WP_Error', wp_count_terms());
assertType('0|numeric-string|WP_Error', wp_count_terms([]));
assertType('0|numeric-string|WP_Error', wp_count_terms(['key' => 'value']));
assertType('0|numeric-string|WP_Error', wp_count_terms(Faker::strArray()));
