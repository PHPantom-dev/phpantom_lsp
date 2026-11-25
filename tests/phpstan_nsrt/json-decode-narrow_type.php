<?php

namespace Analyser\JsonDecode;

use function PHPStan\Testing\assertType;

$value = json_decode('true');
assertType('true', $value);

$value = json_decode('1');
assertType('1', $value);

$value = json_decode('1.5');
assertType('1.5', $value);

$value = json_decode('false');
assertType('false', $value);

$value = json_decode('{}');
assertType('stdClass', $value);

$value = json_decode('[1, 2, 3]');
assertType('array{1, 2, 3}', $value);


function ($mixed) {
	$value = json_decode($mixed);
	assertType('mixed', $value);
};

function ($mixed) {
	$value = json_decode($mixed, false);
	assertType('mixed', $value);
};

function ($mixed, $asArray) {
	$value = json_decode($mixed, $asArray);
	assertType('mixed', $value);
};

function(string $json, ?bool $asArray): void {
	/** @var '{}'|'null' $json */
	$value = json_decode($json);
	assertType('stdClass|null', $value);

	$value = json_decode($json, true);
	assertType('array{}|null', $value);

	$value = json_decode($json, $asArray);
	assertType('array{}|stdClass|null', $value);

	$value = json_decode($json, 'foo');
	assertType('stdClass|null', $value); // SKIP: a non-bool $associative is coerced or a TypeError depending on strict_types, so the declared type is kept
};
