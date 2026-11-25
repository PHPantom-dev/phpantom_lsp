<?php declare(strict_types = 1);

namespace Bug7913b;

use function PHPStan\Testing\assertType;

const X = [];
assertType('array{}', X);
if (!empty(X)) {
	assertType('*NEVER*', X); // SKIP: a branch that cannot run keeps the type instead of `*NEVER*`
	foreach (X as $y) {
		print($y);
	}
}
assertType('array{}', X);

$x = [];
assertType('array{}', $x);
if (!empty($x)) {
	assertType('*NEVER*', $x);
	foreach ($x as $y) {
		print($y);
	}
}
assertType('array{}', $x);
