<?php // lint >= 8.0

namespace MisleadingTypes;

use function PHPStan\Testing\assertType;

class Foo
{

	public function misleadingBoolReturnType(): \MisleadingTypes\boolean
	{

	}

	public function misleadingIntReturnType(): \MisleadingTypes\integer
	{

	}

	public function misleadingMixedReturnType(): mixed
	{

	}

}

function () {
	$foo = new Foo();
	assertType('MisleadingTypes\boolean', $foo->misleadingBoolReturnType());
	assertType('MisleadingTypes\integer', $foo->misleadingIntReturnType());
	assertType('mixed', $foo->misleadingMixedReturnType());
};
