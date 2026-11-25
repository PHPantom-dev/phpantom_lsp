<?php

namespace Analyser\Bug2822;

use function PHPStan\Testing\assertType;

$getter = function (string $key) use ($store): int
{
	return $store[$key];
};

function (array $tokens) {
	$i = 0;
	\assert($tokens[$i] === 1);
	assertType('1', $tokens[$i]);
	$i++;
	assertType('mixed', $tokens[$i]);
};

function () use ($getter) {
	$key = 'foo';

	if ($getter($key) > 4) {
		$key = 'bar';
		assertType('int', $getter($key));
	}
};

function (array $tokens) {
	$lastContent = 'a';

	if ($tokens[$lastContent]['code'] === 1
		|| $tokens[$lastContent]['code'] === 2
	) {
		assertType('1|2', $tokens[$lastContent]['code']);
		$lastContent = 'b';
		assertType('mixed', $tokens[$lastContent]['code']);
		if ($tokens[$lastContent]['code'] === 3) {
			echo "what?";
		}
	}
};
