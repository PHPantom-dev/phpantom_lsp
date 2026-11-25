<?php

namespace Bug5529;

use function PHPStan\Testing\assertType;

class ParentClass
{}

class HelloWorld extends ParentClass
{
	public function run(): ?parent
	{
		if (rand(0,1)) {
			return $this;
		}

		return null;
	}
}

function (HelloWorld $hw): void {
	assertType(ParentClass::class . '|null', $hw->run());
};
