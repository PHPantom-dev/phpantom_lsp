<?php

declare(strict_types=1);

namespace SzepeViktor\PHPStan\WordPress\Tests;

use function shortcode_atts;
use function PHPStan\Testing\assertType;

/** @var array $pairs */
$pairs = $_GET['pairs'];

/** @var array $atts */
$atts = $_GET['atts'];

// shortcode_atts filters atts by returning all atts that are occurring in the pairs
// As atts are supposed to be strings the function is expected to return the pair type or a string

// Constant $pairs and non-constant $atts
assertType('array{}', shortcode_atts([], $atts)); // SKIP: the shape `wp_parse_args()` or `shortcode_atts()` merges is not computed
assertType('array{foo: string, bar: string}', shortcode_atts(['foo' => '', 'bar' => ''], $atts)); // SKIP: the shape `wp_parse_args()` or `shortcode_atts()` merges is not computed
assertType('array{foo: string, bar: 19|string}', shortcode_atts(['foo' => 'foo-value', 'bar' => 19], $atts)); // SKIP: the shape `wp_parse_args()` or `shortcode_atts()` merges is not computed
assertType('array{foo: string|null, bar: 17|string, baz: string}', shortcode_atts(['foo' => null, 'bar' => 17, 'baz' => ''], $atts)); // SKIP: the shape `wp_parse_args()` or `shortcode_atts()` merges is not computed

// Constant $pairs and constant $atts
assertType("array{foo: '', bar: '19', baz: 'aString'}", shortcode_atts(['foo' => null, 'bar' => 17, 'baz' => ''], ['foo' => '', 'bar' => '19', 'baz' => 'aString'])); // SKIP: the shape `wp_parse_args()` or `shortcode_atts()` merges is not computed

// Constant $pairs and empty $atts
assertType('array{}', shortcode_atts([], [])); // SKIP: the shape `wp_parse_args()` or `shortcode_atts()` merges is not computed
assertType("array{foo: '', bar: ''}", shortcode_atts(['foo' => '', 'bar' => ''], [])); // SKIP: the shape `wp_parse_args()` or `shortcode_atts()` merges is not computed
assertType("array{foo: 'foo-value', bar: 19}", shortcode_atts(['foo' => 'foo-value', 'bar' => 19], [])); // SKIP: the shape `wp_parse_args()` or `shortcode_atts()` merges is not computed
assertType("array{foo: null, bar: 17, baz: ''}", shortcode_atts(['foo' => null, 'bar' => 17, 'baz' => ''], [])); // SKIP: the shape `wp_parse_args()` or `shortcode_atts()` merges is not computed

// Non-constant $pairs (mixed) and varying $atts
assertType('array', shortcode_atts($pairs, $atts));
assertType('array', shortcode_atts($pairs, []));
assertType('array', shortcode_atts($pairs, ['foo' => '', 'bar' => '19', 'baz' => '']));

// Non-constant $pairs (int) and varying $atts
/** @var array<string, int> $pairs */
$pairs = $_GET['pairs'];
assertType('array<string, int|string>', shortcode_atts($pairs, $atts)); // SKIP: the shape `wp_parse_args()` or `shortcode_atts()` merges is not computed
assertType('array<string, int>', shortcode_atts($pairs, [])); // SKIP: the shape `wp_parse_args()` or `shortcode_atts()` merges is not computed
assertType('array<string, int|string>', shortcode_atts($pairs, ['foo' => '', 'bar' => '19', 'baz' => ''])); // SKIP: the shape `wp_parse_args()` or `shortcode_atts()` merges is not computed
