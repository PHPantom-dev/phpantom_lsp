<?php

use function PHPStan\Testing\assertType;

$x = null;

/** @var string[] $arr */
$arr = doFoo();
foreach ($arr as $foo) {
	$x = $foo;
}

$y = null;
if (doFoo()) {

} else {
	if (doBar()) {

	} else {
		$y = 1;
	}
}

assertType('string|null', $x);
assertType('1|null', $y);
