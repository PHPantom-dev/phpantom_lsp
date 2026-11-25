<?php

namespace SwitchInstanceOfFallthrough;

use function PHPStan\Testing\assertType;

class Foo
{

	/**
	 * @param object $object
	 */
	public function doFoo($object)
	{
		switch (true) {
			case $object instanceof A:
			case $object instanceof B:
				assertType('SwitchInstanceOfFallthrough\A|SwitchInstanceOfFallthrough\B', $object);
		}
	}

}
