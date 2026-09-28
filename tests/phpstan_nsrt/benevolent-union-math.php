<?php

namespace BenevolentUnionMath;

use function PHPStan\Testing\assertType;
use function strtotime;

class Foo
{

	public function doFoo($s, $r)
	{
		$timeS = strtotime($s);
		assertType('(int|false)', $timeS);

		$timeR = strtotime($r);
		assertType('(int|false)', $timeR);

		assertType('int', $timeS - $timeR);
	}

}

class HelloWorld
{
	public function sayHello(int $x): void
	{
		$dbresponse = $this->getBenevolent();
		assertType('array<string, (float|int|string|null)>|null', $dbresponse);

		if ($dbresponse === null) {return;}

		assertType('array<string, (float|int|string|null)>', $dbresponse);
		assertType('(float|int|string|null)', $dbresponse['Value']);
		assertType('int<0, max>', strlen($dbresponse['Value']));
	}

	/**
	 * @return array<string, __benevolent<float|int|string|null>>|null
	 */
	private function getBenevolent(): ?array{
		return rand(10) > 1 ? null : [];
	}
}
