<?php

use PHPStan\TrinaryLogic;
use function PHPStan\Testing\assertType;
use function PHPStan\Testing\assertVariableCertainty;

assertVariableCertainty(TrinaryLogic::createMaybe(), $foo);

/** @var Foo $foo */

assertVariableCertainty(TrinaryLogic::createYes(), $foo);

function (): void {
	assertVariableCertainty(TrinaryLogic::createNo(), $foo);

	/** @var Foo $foo */

	assertVariableCertainty(TrinaryLogic::createNo(), $foo);
};

function (): void {
	if (rand(0, 1) === 0) {
		$foo = doFoo();
	}

	assertVariableCertainty(TrinaryLogic::createMaybe(), $foo);

	/** @var Foo $foo */

	assertVariableCertainty(TrinaryLogic::createMaybe(), $foo);
};

assertVariableCertainty(TrinaryLogic::createMaybe(), $bar);
assert($bar instanceof Foo);
assertVariableCertainty(TrinaryLogic::createYes(), $bar);

function (): void {
	assertVariableCertainty(TrinaryLogic::createNo(), $bar);

	assert($bar instanceof Foo);

	assertVariableCertainty(TrinaryLogic::createYes(), $bar);
};

/** @var Foo $lorem */
/** @var Bar $ipsum */

assertVariableCertainty(TrinaryLogic::createYes(), $lorem);
assertType('Foo', $lorem);
assertVariableCertainty(TrinaryLogic::createYes(), $ipsum);
assertType('Bar', $ipsum);
