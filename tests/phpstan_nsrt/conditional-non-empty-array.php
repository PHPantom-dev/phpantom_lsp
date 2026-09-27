<?php

namespace ConditionalNonEmptyArray;

use PHPStan\TrinaryLogic;
use function PHPStan\Testing\assertType;
use function PHPStan\Testing\assertVariableCertainty;

class Foo
{

	public function doFoo(array $a): void
	{
		foreach ($a as $val) {
			$foo = 1;
		}

		assertType('array', $a);
		assertVariableCertainty(TrinaryLogic::createMaybe(), $foo);

		if (count($a) > 0) {
			assertType('non-empty-array', $a);
			assertVariableCertainty(TrinaryLogic::createYes(), $foo);
		} else {
			assertType('array{}', $a);
			assertVariableCertainty(TrinaryLogic::createNo(), $foo);
		}
	}

}
