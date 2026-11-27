<?php // lint >= 7.4

declare(strict_types = 1);

namespace Bug12211;

use function PHPStan\Testing\assertType;

const REGEX = '((m.x))';

function foo(string $text): void {
	assert(preg_match(REGEX, $text, $match) === 1);
	assertType('array{string, string}', $match);
}


