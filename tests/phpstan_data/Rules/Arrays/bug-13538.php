<?php

namespace Bug13538;

use LogicException;
use function PHPStan\Testing\assertType;

/** @param list<string> $arr */
function doFoo(array $arr, int $i, int $i2): void
{
	$logs = [];
	$logs[$i] = '';
	echo $logs[$i2];

	assertType("''", $logs[$i]);
	assertType("''", $logs[$i2]); // could be mixed // SKIP: writing a literal into an array offset widens it to its base type

	foreach ($arr as $value) {
		echo $logs[$i];

		assertType("''", $logs[$i]);
	}
}

/** @param list<string> $arr */
function doFooBar(array $arr): void
{
	if (!defined('LOG_DIR')) {
		throw new LogicException();
	}

	$logs = [];
	$logs[LOG_DIR] = '';

	assertType("''", $logs[LOG_DIR]); // SKIP: writing a literal into an array offset widens it to its base type

	foreach ($arr as $value) {
		echo $logs[LOG_DIR];

		assertType("''", $logs[LOG_DIR]); // SKIP: writing a literal into an array offset widens it to its base type
	}
}

function doBar(array $arr, int $i, string $s): void
{
	$logs = [];
	$logs[$i][$s] = '';
	assertType("''", $logs[$i][$s]);
	foreach ($arr as $value) {
		assertType("''", $logs[$i][$s]);
		echo $logs[$i][$s];
	}
}
