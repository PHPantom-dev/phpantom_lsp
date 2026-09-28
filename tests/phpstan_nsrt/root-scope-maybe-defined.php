<?php

use PHPStan\TrinaryLogic;

\PHPStan\Testing\assertType('mixed', $foo);
\PHPStan\Testing\assertVariableCertainty(TrinaryLogic::createMaybe(), $foo);

$bar = 'str';
\PHPStan\Testing\assertVariableCertainty(TrinaryLogic::createYes(), $bar);
\PHPStan\Testing\assertVariableCertainty(TrinaryLogic::createMaybe(), $baz);

\PHPStan\Testing\assertType('\'str\'', $bar);

if (!isset($baz)) {
	$baz = 1;
	\PHPStan\Testing\assertType('1', $baz);
}


function () {
	\PHPStan\Testing\assertVariableCertainty(TrinaryLogic::createNo(), $foo);
};
