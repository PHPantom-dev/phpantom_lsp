<?php // lint >= 8.0

namespace GetenvPHP80;

use function PHPStan\Testing\assertType;

class Foo
{

	public function test(string|null $stringOrNull, mixed $mixed)
	{
		assertType('array<string, string>', getenv(null));
		assertType('array<string, string>', getenv());
		assertType('string|false', getenv('foo'));

		assertType('array<string, string>|string|false', getenv($stringOrNull));
		assertType('array<string, string>|string|false', getenv($mixed));
	}

}
