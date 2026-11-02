<?php

/**
 * Test data for WpParseUrlFunctionDynamicReturnTypeExtension.
 */

declare(strict_types=1);

namespace SzepeViktor\PHPStan\WordPress\Tests;

use function wp_parse_url;
use function PHPStan\Testing\assertType;

/** @var int $integer */
$integer = doFoo();

/** @var string $string */
$string = doFoo();

$value = wp_parse_url('http://abc.def');
assertType("array{scheme: 'http', host: 'abc.def'}", $value);

$value = wp_parse_url('http://def.abc', -1);
assertType("array{scheme: 'http', host: 'def.abc'}", $value);

$value = wp_parse_url('http://def.abc', $integer);
assertType('array{scheme?: string, host?: string, port?: int<0, 65535>, user?: string, pass?: string, path?: string, query?: string, fragment?: string}|int<0, 65535>|string|false|null', $value);

$value = wp_parse_url('http://def.abc', PHP_URL_FRAGMENT);
assertType('null', $value);

$value = wp_parse_url('http://def.abc#this-is-fragment', PHP_URL_FRAGMENT);
assertType("'this-is-fragment'", $value);

// Upstream expects `false`, but WordPress gives `null` for a component it
// does not know, and `false` only for a URL `parse_url()` cannot parse.
$value = wp_parse_url('http://def.abc#this-is-fragment', 9999);
assertType('null', $value);

$value = wp_parse_url($string, 9999);
assertType('false|null', $value);

$value = wp_parse_url($string, PHP_URL_PORT);
assertType('int<0, 65535>|false|null', $value);

$value = wp_parse_url($string);
assertType('array{scheme?: string, host?: string, port?: int<0, 65535>, user?: string, pass?: string, path?: string, query?: string, fragment?: string}|false', $value);

/** @var 'http://def.abc'|'https://example.com' $union */
$union = $union;
assertType("array{scheme: 'http', host: 'def.abc'}|array{scheme: 'https', host: 'example.com'}", wp_parse_url($union));

/** @var 'http://def.abc#fragment1'|'https://example.com#fragment2' $union */
$union = $union;
assertType("'fragment1'|'fragment2'", wp_parse_url($union, PHP_URL_FRAGMENT));
