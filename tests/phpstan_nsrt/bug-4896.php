<?php // lint >= 8.0

declare(strict_types = 1);

namespace Bug4896;

use function PHPStan\Testing\assertType;

class Foo
{

	public function doFoo(\DateTime|\DateInterval $command): void
	{
		switch ($command::class) {
			case \DateTime::class:
				assertType(\DateTime::class, $command);
				break;
			case \DateInterval::class:
				assertType(\DateInterval::class, $command);
				break;
		}

	}

}

class Bar
{

	public function doFoo(\DateTime|\DateInterval $command): void
	{
		match ($command::class) {
			\DateTime::class => assertType(\DateTime::class, $command),
			\DateInterval::class => assertType(\DateInterval::class, $command),
		};
	}

}
