<?php

namespace EmptyIteratorTest;

use function PHPStan\Testing\assertType;

class Foo
{
	public function doFoo(\EmptyIterator $it): void
	{
		assertType('EmptyIterator', $it);
		assertType('never', $it->key());
		assertType('never', $it->current());
		assertType('null', $it->next());
		assertType('false', $it->valid());
	}

}
