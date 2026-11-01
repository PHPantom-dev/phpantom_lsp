<?php

declare(strict_types=1);

namespace SzepeViktor\PHPStan\WordPress\Tests;

use stdClass;

use function PHPStan\Testing\assertType;

// Indexed array as parameter
assertType('array{string, string}', esc_sql(['someValue', 'toEscape'])); // SKIP: `esc_sql()` does not keep the shape of the array it escapes

// Associative array as parameter
assertType('array{someValue: string}', esc_sql(['someValue' => 'toEscape'])); // SKIP: `esc_sql()` does not keep the shape of the array it escapes

// String as parameter
assertType('string', esc_sql('someValueToEscape'));

// Wrong type provided
assertType("''", esc_sql(new stdClass()));
assertType("''", esc_sql(null));
assertType("array{''}", esc_sql([null])); // SKIP: `esc_sql()` does not keep the shape of the array it escapes

assertType('string', esc_sql(true));
assertType('array{string}', esc_sql([true])); // SKIP: `esc_sql()` does not keep the shape of the array it escapes
assertType('string', esc_sql(1));
assertType('array{array{string}}', esc_sql([[1]])); // SKIP: `esc_sql()` does not keep the shape of the array it escapes
assertType('array{array{key: string}}', esc_sql([['key' => 1]])); // SKIP: `esc_sql()` does not keep the shape of the array it escapes
assertType('array{key: array{string}}', esc_sql(['key' => [1]])); // SKIP: `esc_sql()` does not keep the shape of the array it escapes
assertType('array{key1: array{key2: string}}', esc_sql(['key1' => ['key2' => 1]])); // SKIP: `esc_sql()` does not keep the shape of the array it escapes

/** @var array{foo?: 'something'}|array{bar: 'something else'} $union1 */
$union1 = null;
assertType('array{bar: string}|array{foo?: string}', esc_sql($union1)); // SKIP: `esc_sql()` does not keep the shape of the array it escapes

/** @var 'foo'|array{foo?: 'bar'} $union2 */
$union2 = null;
assertType('array{foo?: string}|string', esc_sql($union2)); // SKIP: `esc_sql()` does not keep the shape of the array it escapes

/** @var array{foo?: 'bar'}|null $union3 */
$union3 = null;
assertType("''|array{foo?: string}", esc_sql($union3)); // SKIP: `esc_sql()` does not keep the shape of the array it escapes

/** @var mixed $mixed */
$mixed = null;
assertType('array|string', esc_sql($mixed));

