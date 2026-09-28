<?php

namespace Bug2549;

use PHPStan\TrinaryLogic;
use function PHPStan\Testing\assertType;
use function PHPStan\Testing\assertVariableCertainty;

class Pear
{

	public function doFoo(): void
	{
		$apples = [1, 2, 3];

		foreach($apples as $apple) {
			$pear = null;

			switch(true) {
				case true:
					$pear = new self();

					break;

				default:
					continue 2;
			}

			assertType('Bug2549\Pear', $pear);
		}
	}

	public function sayHello(): void
	{
		$array = [1,2,3];

		foreach($array as $value) {
			switch ($value) {
				case 1:
					$a = 2;
					break;
				case 2:
					$a = 3;
					break;
				default:
					continue 2;
			}

			assertType('2|3', $a);
			assertVariableCertainty(TrinaryLogic::createYes(), $a);
		}
	}
}
