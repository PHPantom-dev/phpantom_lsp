<?php

declare(strict_types=1);

namespace SzepeViktor\PHPStan\WordPress\Tests;

use function wp_slash;
use function PHPStan\Testing\assertType;

assertType("''", wp_slash('')); // SKIP: a call on literal arguments is not evaluated
assertType("'foo'", wp_slash('foo')); // SKIP: a call on literal arguments is not evaluated
assertType("'foo\\\\\'s bar'", wp_slash("foo's bar")); // SKIP: a call on literal arguments is not evaluated

/** @var string $value */
assertType('string', wp_slash($value));

/** @var lowercase-string $value */
assertType('lowercase-string', wp_slash($value)); // SKIP: `wp_slash()` does not keep a `lowercase-string`

assertType("'baz'|'foo\\\\\'s bar'", wp_slash(rand(0, 1) === 1 ? 'baz' : "foo's bar")); // SKIP: a call on literal arguments is not evaluated

assertType("array{who: 'baz', where: 'foo\\\\\'s bar'}", wp_slash(['who' => 'baz', 'where' => "foo's bar"])); // SKIP: `wp_slash()` does not keep the shape of the array it slashes

/** @var array{who: 'baz', when: non-falsy-string, where: "foo's bar"} $value */
assertType("array{who: 'baz', when: non-falsy-string, where: 'foo\\\\\'s bar'}", wp_slash($value)); // SKIP: `wp_slash()` does not keep the shape of the array it slashes

assertType("array{outer: array{inner: 'foo\\\\\'s bar'}}", wp_slash(['outer' => ['inner' => "foo's bar"]])); // SKIP: `wp_slash()` does not keep the shape of the array it slashes

$value = rand(0, 1) === 1
	? ['outer' => ['inner' => "foo's bar"]]
	: ['outer' => ['inner' => 'baz']];
assertType("array{outer: array{inner: 'baz'}}|array{outer: array{inner: 'foo\\\\\'s bar'}}", wp_slash($value)); // SKIP: `wp_slash()` does not keep the shape of the array it slashes

/** @var array<array{inner: "foo's bar"}> $value */
assertType("array<array{inner: 'foo\\\\\'s bar'}>", wp_slash($value)); // SKIP: `wp_slash()` does not keep the shape of the array it slashes

// Nested array with multiple items at each depth
assertType( // SKIP: `wp_slash()` does not keep the shape of the array it slashes
	"array{level1_a: array{level2_a: 'foo\\\\\'s bar', level2_b: 'level2_b'}, level1_b: array{level2_c: array{level3_a: 'deeper foo\\\\\'s bar', level3_b: 'level3_b'}, level2_d: 'level2_d'}}",
	wp_slash([
		'level1_a' => [
			'level2_a' => "foo's bar",
			'level2_b' => 'level2_b'
		],
		'level1_b' => [
			'level2_c' => [
				'level3_a' => "deeper foo's bar",
				'level3_b' => 'level3_b'
			],
			'level2_d' => 'level2_d'
		]
	])
);
