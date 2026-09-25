<?php

namespace OverridingPhpDocTypeOfProtectedProperty;

use stdClass;
use function PHPStan\Testing\assertType;

class Foo
{

	/** @var array|object */
	protected $config;

}

/**
 * @property stdClass $config
 */
class Bar extends Foo
{

	public function doFoo()
	{
		assertType(stdClass::class, $this->config);
	}

}
