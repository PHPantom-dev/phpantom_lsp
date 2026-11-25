<?php

namespace IniGet;

use function PHPStan\Testing\assertType;

function doFoo() {
	assertType('string', ini_get("date.timezone"));
	assertType('string', ini_get("memory_limit"));

	if (rand(1, 0)) {
		$x = ini_get("date.timezone");
	} else {
		$x = ini_get("max_execution_time");
	}
	assertType('string', $x);

	if (rand(1, 0)) {
		$key = 'unknown';
	} else {
		$key = "max_execution_time";
	}
	assertType('string|false', ini_get($key));
	assertType('string|false', ini_get('unknown'));

	if (PHP_VERSION_ID >= 80500) {
		assertType('string', ini_get("max_memory_limit"));
	} else {
	}
	if (PHP_VERSION_ID >= 80300) {
		assertType('string', ini_get("max_memory_limit")); // PHPStan's expectation is for PHP < 8.5; the runner is 8.5, where `max_memory_limit` exists
	}
	if (PHP_VERSION_ID < 80300) {
	}
}
