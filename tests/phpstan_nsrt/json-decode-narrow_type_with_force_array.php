<?php

namespace Analyser\JsonDecode;

use function PHPStan\Testing\assertType;

$value = json_decode('true', true);
assertType('true', $value);

$value = json_decode('1', true);
assertType('1', $value);

$value = json_decode('1.5', true);
assertType('1.5', $value);

$value = json_decode('false', true);
assertType('false', $value);

$value = json_decode('{}', true);
assertType('array{}', $value);

$value = json_decode('[1, 2, 3]', true);
assertType('array{1, 2, 3}', $value);

function ($mixed) {
	$value = json_decode($mixed, true);
	assertType('mixed~object', $value); // SKIP: json_decode() of an unknown string into arrays does not rule out objects
};
