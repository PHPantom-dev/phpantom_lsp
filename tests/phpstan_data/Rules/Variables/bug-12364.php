<?php

namespace Bug12364;

use PHPStan\TrinaryLogic;
use function PHPStan\Testing\assertType;
use function PHPStan\Testing\assertVariableCertainty;

/** @return array{x: string, y?: string, z?: string} */
function foo(): array {
	return [ 'x' => 'foo' ];
}

$x = $y = null;
assertType('null', $x);
assertType('null', $y);
extract(foo());
assertType('string', $x);
assertType('string|null', $y); // <-- should be: null|string
// PHPStan says `mixed` because a global-scope variable it has not seen is
// already maybe-defined as `mixed`; the optional key can only write a string.
assertType('string', $z);
assertVariableCertainty(TrinaryLogic::createYes(), $x);
assertVariableCertainty(TrinaryLogic::createYes(), $y);
assertVariableCertainty(TrinaryLogic::createMaybe(), $z);
var_dump($x);
var_dump($y); // <-- does exist

/** @return array{xx: string, yy?: string} */
function foo2(): array {
	return [ 'xx' => 'foo' ];
}

function testUndefined()
{
	if (rand(0, 1)) {
		$xx = $yy = 0;
		assertType('0', $xx);
		assertType('0', $yy);
	}

	extract(foo2());
	assertType('string', $xx);

	if (isset($yy)) {
		assertType('0|string', $yy);
	}
}
