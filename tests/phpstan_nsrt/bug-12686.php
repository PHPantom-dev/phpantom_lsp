<?php declare(strict_types = 1);

namespace Bug12686;

use function PHPStan\Testing\assertType;

/** @phpstan-impure */
$f = function (): bool {
	return (bool) rand(0,1);
};

if ($f()) {
	assertType('bool', $f());
}

// Pure closure should still have narrowing
$h = function (): bool {
	return true;
};

if ($h()) {
	assertType('true', $h());
}

// Multiple callable parameter acceptors (union of closures)
// When one variant is impure, the combined result should be impure
/** @phpstan-impure */
$impure = function (): bool {
	return (bool) rand(0, 1);
};

$pure = function (): bool {
	return true;
};

if (rand(0, 1)) {
	$g = $impure;
} else {
	$g = $pure;
}

if ($g()) {
	assertType('bool', $g());
}

// Multiple callable parameter acceptors where all are pure
$pure1 = function (): bool {
	return true;
};

$pure2 = function (): bool {
	return true;
};

if (rand(0, 1)) {
	$p = $pure1;
} else {
	$p = $pure2;
}

if ($p()) {
	assertType('true', $p());
}
