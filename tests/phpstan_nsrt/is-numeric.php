<?php

function () {
	/** @var int|string $s */
	$s = doFoo();
	if (!is_numeric($s)) {
		\PHPStan\Testing\assertType('string', $s);
	}
};
