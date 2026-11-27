<?php // lint >= 7.4

declare(strict_types = 1);

namespace Bug12210;

use function PHPStan\Testing\assertType;

function bug12210a(string $text): void {
	assert(preg_match('(((sum|min|max)))', $text, $match) === 1);
	assertType("array{string, 'max'|'min'|'sum', 'max'|'min'|'sum'}", $match);
}

function bug12210b(string $text): void {
	assert(preg_match('(((sum|min|ma.)))', $text, $match) === 1);
	assertType("array{string, string, string}", $match);
}

function bug12210c(string $text): void {
	assert(preg_match('(((su.|min|max)))', $text, $match) === 1);
	assertType("array{string, string, string}", $match);
}

function bug12210d(string $text): void {
	assert(preg_match('(((sum|mi.|max)))', $text, $match) === 1);
	assertType("array{string, string, string}", $match);
}
