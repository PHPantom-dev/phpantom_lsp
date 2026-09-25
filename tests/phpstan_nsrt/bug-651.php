<?php

namespace Bug651;

use PHPStan\TrinaryLogic;
use function PHPStan\Testing\assertType;
use function PHPStan\Testing\assertVariableCertainty;

function (): void {
	foreach (['foo', 'bar'] as $loopValue) {
		switch ($loopValue) {
			case 'foo':
				continue 2;

			case 'bar':
				$variableDefinedWithinForeach = 23;
				break;

			default:
				throw new \LogicException();
		}

		assertType('23', $variableDefinedWithinForeach);
		assertVariableCertainty(TrinaryLogic::createYes(), $variableDefinedWithinForeach);
		echo $variableDefinedWithinForeach;
	}
};
