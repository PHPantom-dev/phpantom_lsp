<?php

namespace Bug6174;

use function PHPStan\Testing\assertType;

class HelloWorld
{
	private const DEFAULT_VALUE = 10;

	public function __construct()
	{
		$tempValue = (int) ($this->returnValue() ?? self::DEFAULT_VALUE);
		assertType('-1|int<1, max>', $tempValue === -1 || $tempValue > 0 ? $tempValue : self::DEFAULT_VALUE);
	}

	public function returnValue(): ?string
	{
		return null;
	}
}
