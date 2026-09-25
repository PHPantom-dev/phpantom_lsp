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
		assertType('null', $it->next()); // SKIP: a void call's result is typed void instead of null
		assertType('false', $it->valid());
	}

}
