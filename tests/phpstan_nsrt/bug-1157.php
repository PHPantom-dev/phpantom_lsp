<?php

namespace Bug1157;

use DateTimeInterface;
use function PHPStan\Testing\assertType;

class HelloWorld
{
	private $a;

	/**
	 * @return \DateTimeInterface|null
	 */
	public function getA()
	{
		return $this->a;
	}
}

function (HelloWorld $class): void {
	if ($class->getA()) {
		assertType(DateTimeInterface::class, $class->getA());
	}
};
