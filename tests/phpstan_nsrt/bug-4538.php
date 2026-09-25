<?php

namespace Bug4538;

use function PHPStan\Testing\assertType;

class Foo
{
	/**
	 * @param string $index
	 */
	public function bar(string $index): void
	{
		assertType('string|false', getenv($index));
		assertType('array<string, string>', getenv());
	}
}
