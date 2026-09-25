<?php

namespace Bug3331;

use function PHPStan\Testing\assertType;

class HelloWorld
{
	public function inline(): void
	{
		if (is_object($obj = rand(0, 1) ? null : new \stdClass())) {
			assertType(\stdClass::class, $obj);
		}
	}

	public function perline(): void
	{
		$obj = rand(0, 1) ? null : new \stdClass();
		if (is_object($obj)) {
			assertType(\stdClass::class, $obj);
		}
	}
}
