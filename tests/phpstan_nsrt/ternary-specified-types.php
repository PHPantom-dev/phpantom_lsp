<?php

namespace TernarySpecifiedTypes;

use function PHPStan\Testing\assertType;

class Foo
{

	public function doFoo(bool $a, bool $b)
	{
		if ($a ? $b : false) {
			// $a && $b
		} else {
			// !$a || !$b
			assertType('bool', $a);
			assertType('bool', $b);
			if (!$a) {
				assertType('false', $a);
				assertType('bool', $b);
			} else {
				assertType('true', $a);
			}
		}
	}

	public function doBar(bool $a)
	{
		if ($a ?: false) {
		} else {
		}
	}

}
