<?php

namespace Bug4700;

use function PHPStan\Testing\assertType;

function(array $array, int $count): void {
	if ($count < 1) {
		return;
	}

	assertType('int<1, max>', $count);

	$a = [];
	if (isset($array['a'])) $a[] = $array['a'];
	if (isset($array['b'])) $a[] = $array['b'];
	if (isset($array['c'])) $a[] = $array['c'];
	if (isset($array['d'])) $a[] = $array['d'];
	if (isset($array['e'])) $a[] = $array['e'];
	if (count($a) >= $count) {
	} else {
	}
};

function(array $array, int $count): void {
	if ($count < 1) {
		return;
	}

	assertType('int<1, max>', $count);

	$a = [];
	if (isset($array['a'])) $a[] = $array['a'];
	if (isset($array['b'])) $a[] = $array['b'];
	if (isset($array['c'])) $a[] = $array['c'];
	if (isset($array['d'])) $a[] = $array['d'];
	if (isset($array['e'])) $a[] = $array['e'];
	if (count($a) > $count) {
	} else {
		assertType('int<0, 5>', count($a)); // Could be int<0, 1>
	}
};
