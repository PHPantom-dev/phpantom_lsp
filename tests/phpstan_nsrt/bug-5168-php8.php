<?php // lint >= 8.0

namespace Bug5168Php8;

use function PHPStan\Testing\assertType;

function (float $f): void {
	define('LARAVEL_START', microtime(true));

	$comment = 'Calculated in ' . microtime(true) - $f;
	assertType('non-falsy-string', $comment);
};
