<?php // lint >= 8.0

namespace Bug9764;

use function PHPStan\Testing\assertType;

/**
 * @template T
 * @param callable(): T $fnc
 * @return T
 */
function result(callable $fnc): mixed
{
	return $fnc();
}

function (): void {
	/** @var array<non-empty-string, string> $a */
	$a = [];
	$c = static fn (): array => $a;
	assertType('Closure(): array<non-empty-string, string>', $c);

	$r = result($c);
	assertType('array<non-empty-string, string>', $r);
};
