<?php

namespace Bug801;

use function PHPStan\Testing\assertType;

class HelloWorld
{
	public function getDateTime(): ?\DateTime
	{
		return (bool) rand(0, 1) ? new \DateTime('now') : null;
	}
}

function (): void {
	$hello = new HelloWorld();
	$dt = $hello->getDateTime();

	$condition = null !== $dt;

	if ($condition) {
		assertType('DateTime', $dt);
	} else {
		assertType('null', $dt);
	}
};
