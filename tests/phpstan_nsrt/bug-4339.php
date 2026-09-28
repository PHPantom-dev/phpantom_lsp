<?php

namespace Bug4339;

use function PHPStan\Testing\assertType;

function (?string $v) {
	assertType('string', $v ?? '-');
	fn (?string $value): string => assertType('string', $value ?? '-');
	fn (?string $value): void => assertType('string|null', $value);

	$f = fn (?string $value): string => $value ?? '-';

	assertType('string', $f($v));
};
