<?php declare(strict_types = 1);

namespace Bug8827;

use function PHPStan\Testing\assertType;

class HelloWorld
{
	public function test(): void
	{
		$efferent = $afferent = 0;
		$nbElements = random_int(0, 30);

		$elements = array_fill(0, $nbElements, random_int(0, 2));

		foreach ($elements as $element)
		{
			$efferent += ($element === 1);
			$afferent += ($element === 2);
		}

		assertType('int<0, max>', $efferent); // Expected: int<0, $nbElements> | Actual: 0|1
		assertType('int<0, max>', $afferent); // Expected: int<0, $nbElements> | Actual: 0|1

		$instability = ($efferent + $afferent > 0) ? $efferent / ($afferent + $efferent) : 0;
	}
}
