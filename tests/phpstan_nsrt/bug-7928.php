<?php declare(strict_types = 1);

namespace Bug7928;

use function PHPStan\Testing\assertType;

function findEntry(): ?string
{
	$array = [
		randomEntry(),
		randomEntry(),
	];
	$array = array_filter($array);
	$first = reset($array);
	assertType('non-falsy-string|false', $first);
	if ($first === false) {
		return null;
	}
	return $first;
};

function randomEntry(): ?string
{
	$rand = mt_rand(0, 1);
	if ($rand === 1) {
		return 'foo';
	}
	return null;
}
