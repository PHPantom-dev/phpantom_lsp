<?php

use function PHPStan\Testing\assertType;

$integers = [0, 1, 2, 3];
$strings = ['foo', 'bar'];
$emptyArray = [];
$mixedArray = [0, 'foo'];

$nestedArray = [
	'foo' => [
		'foo' => [
			'foo' => 'bar',
		],
	],
	'bar' => [],
	'baz' => [
		'lorem' => [],
	],
];

assertType('0', $integers[0]);
assertType('1', $integers[1]);
assertType('\'foo\'', $strings[0]);
assertType('0', $mixedArray[0]);
assertType('array{foo: array{foo: array{foo: \'bar\'}}, bar: array{}, baz: array{lorem: array{}}}', $nestedArray);
assertType('0', $integers['0']);
