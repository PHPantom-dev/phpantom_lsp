<?php

declare(strict_types=1);

namespace SzepeViktor\PHPStan\WordPress\Tests;

use function wp_slash;
use function PHPStan\Testing\assertType;

assertType("''", wp_slash(''));
assertType("'foo'", wp_slash('foo'));
assertType("'foo\\\\\'s bar'", wp_slash("foo's bar"));

/** @var string $value */
assertType('string', wp_slash($value));

/** @var lowercase-string $value */
assertType('lowercase-string', wp_slash($value));

assertType("'baz'|'foo\\\\\'s bar'", wp_slash(rand(0, 1) === 1 ? 'baz' : "foo's bar"));

assertType("array{who: 'baz', where: 'foo\\\\\'s bar'}", wp_slash(['who' => 'baz', 'where' => "foo's bar"]));

/** @var array{who: 'baz', when: non-falsy-string, where: "foo's bar"} $value */
assertType("array{who: 'baz', when: non-falsy-string, where: 'foo\\\\\'s bar'}", wp_slash($value));

assertType("array{outer: array{inner: 'foo\\\\\'s bar'}}", wp_slash(['outer' => ['inner' => "foo's bar"]]));

$value = rand(0, 1) === 1
	? ['outer' => ['inner' => "foo's bar"]]
	: ['outer' => ['inner' => 'baz']];
assertType("array{outer: array{inner: 'baz'}}|array{outer: array{inner: 'foo\\\\\'s bar'}}", wp_slash($value));

/** @var array<array{inner: "foo's bar"}> $value */
assertType("array<array{inner: 'foo\\\\\'s bar'}>", wp_slash($value));

// Nested array with multiple items at each depth
assertType(
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
