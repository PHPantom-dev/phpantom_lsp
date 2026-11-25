<?php

namespace Bug4499;

use function PHPStan\Testing\assertType;

class Foo
{

	/** @param list<int> $things */
	function thing(array $things) : void{
		switch(count($things)){
			case 1:
				assertType('array{int}', $things);
				assertType('int', array_shift($things));
		}
	}

}
