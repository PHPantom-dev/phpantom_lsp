<?php

namespace TypeChangeAfterArrayAccessAssignment;

use function PHPStan\Testing\assertType;

class Foo
{

	/**
	 * @param \ArrayAccess<int, int> $ac
	 */
	public function doFoo(\ArrayAccess $ac): void
	{
		assertType('ArrayAccess<int, int>', $ac);

		$ac['foo'] = 'bar';
		assertType('ArrayAccess<int, int>', $ac);

		$ac[] = 'foo';
		assertType('ArrayAccess<int, int>', $ac);

		$ac[] = 1;
		assertType('ArrayAccess<int, int>', $ac);

		$ac[2] = 'bar';
		assertType('ArrayAccess<int, int>', $ac);

		$i = 1;
		$ac[] = $i;
		assertType('ArrayAccess<int, int>', $ac);

		$ac[] = 'baz';
		assertType('ArrayAccess<int, int>', $ac);

		$ac[] = ['foo'];
		assertType('ArrayAccess<int, int>', $ac);
	}

}
