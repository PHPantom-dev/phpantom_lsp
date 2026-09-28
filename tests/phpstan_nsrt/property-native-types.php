<?php

namespace PropertyNativeTypes;

use function PHPStan\Testing\assertType;

class Foo
{

	private string $stringProp;

	private self $selfProp;

	/** @var int[] */
	private array $integersProp;

	public function doFoo()
	{
		assertType('string', $this->stringProp);
		assertType('PropertyNativeTypes\Foo', $this->selfProp);
		assertType('array<int>', $this->integersProp);
	}

}
