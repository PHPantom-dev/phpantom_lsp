<?php

namespace Bug4504TypeInference;

use function PHPStan\Testing\assertType;

class Foo
{

	public function sayHello($models): void
	{
		/** @var \Iterator<A> $models */
		foreach ($models as $k => $v) {
			assertType('Bug4504TypeInference\A', $v);
		}

		assertType('Iterator<mixed, Bug4504TypeInference\A>', $models);
	}

}

