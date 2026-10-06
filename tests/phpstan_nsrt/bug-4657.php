<?php

namespace Bug4657;

use DateTime;
use function PHPStan\Testing\assertType;
use function PHPStan\Testing\assertNativeType;

function (): void {
	$value = null;
	$other = null;
	$callback = function () use (&$value, &$other) : void {
		$value = new DateTime();
	};
	$callback();

	assertType('DateTime|null', $value);
	assertNativeType('DateTime|null', $value);

	assertType('null', $other);
	assertNativeType('null', $other);
};

function (): void {
	$value = null;
	$other = null;
	$callback = function () use (&$value, &$other) : void {
		if (rand(0, 1)) {
			$value = new DateTime();
		}
	};
	$callback();

	assertType('DateTime|null', $value);
	assertNativeType('DateTime|null', $value);

	assertType('null', $other);
	assertNativeType('null', $other);
};

function (): void {
	$value = null;
	$other = null;
	$callback = function () use (&$value, &$other) : void {
		if (rand(0, 1)) {
			return;
		}

		$value = new DateTime();
	};
	$callback();

	assertType('DateTime|null', $value);
	assertNativeType('DateTime|null', $value);

	assertType('null', $other);
	assertNativeType('null', $other);
};
