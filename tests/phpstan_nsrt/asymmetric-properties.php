<?php

namespace AsymmetricProperties;

use AllowDynamicProperties;
use function PHPStan\Testing\assertType;

/**
 * @property-read int $asymmetricPropertyRw
 * @property-write int|string $asymmetricPropertyRw
 *
 * @property int $asymmetricPropertyXw
 * @property-write int|string $asymmetricPropertyXw
 *
 * @property-read int $asymmetricPropertyRx
 * @property int|string $asymmetricPropertyRx
 */
#[AllowDynamicProperties]
class Foo
{

	public function doFoo(): void
	{
		assertType('int', $this->asymmetricPropertyRw);
		assertType('int', $this->asymmetricPropertyXw);
		assertType('int', $this->asymmetricPropertyRx);
	}

}
