<?php

namespace SwitchGetClass;

use function PHPStan\Testing\assertType;

class Foo
{

	public function doFoo()
	{
		$lorem = doFoo();

		switch (get_class($lorem)) {
			case Ipsum::class:
				break;
			case Lorem::class:
				assertType('SwitchGetClass\Lorem', $lorem);
				break;
			case self::class:
				assertType('SwitchGetClass\Foo', $lorem);
				break;
		}
	}

}
