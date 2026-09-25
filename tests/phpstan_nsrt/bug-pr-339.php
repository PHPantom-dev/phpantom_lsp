<?php

namespace BugPr339;

use PHPStan\TrinaryLogic;
use function PHPStan\Testing\assertType;
use function PHPStan\Testing\assertVariableCertainty;

assertVariableCertainty(TrinaryLogic::createMaybe(), $a);
assertVariableCertainty(TrinaryLogic::createMaybe(), $c);
assertType('mixed', $a);
assertType('mixed', $c);

if ($a || $c) {
	assertVariableCertainty(TrinaryLogic::createMaybe(), $a);
	assertVariableCertainty(TrinaryLogic::createMaybe(), $c);
	assertType('mixed', $a);
	assertType('mixed', $c);
	if ($a) {
		assertType('mixed', $c);
		assertVariableCertainty(TrinaryLogic::createYes(), $a);
	}

	if ($c) {
		assertType('mixed', $a);
		assertVariableCertainty(TrinaryLogic::createYes(), $c);
	}
} else {
}
