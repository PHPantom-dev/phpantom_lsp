<?php

namespace Bug10699;

use function PHPStan\Testing\assertType;

/**
 * @param int $flags
 * @param mixed $out
 *
 * @param-out ($flags is 2 ? 20 : 10) $out
 *
 * @return ($flags is 2 ? 20 : 10)
 */
function test(int $flags, &$out): int
{
	$out = $flags === 2 ? 20 : 10;

	return $out;
}

function (): void {
	$res = test(1, $out);
	assertType('10', $res);
	assertType('10', $out);

	$res = test(2, $out);
	assertType('20', $res);
	assertType('20', $out);
};

/**
 * @param int $flags
 * @param mixed $out
 *
 * @param-out ($flags is 2 ? 20 : 10) $out
 */
function test2(int $flags, &$out): void
{
	$out = $flags === 2 ? 20 : 10;
}

function (): void {
	test2(1, $out);
	assertType('10', $out);

	test2(2, $out);
	assertType('20', $out);
};
