<?php

use function PHPStan\Testing\assertType;

class FooClassForNodeScopeResolverTestingWithoutNamespace
{

	public function misleadingBoolReturnType(): \boolean
	{

	}

	public function misleadingIntReturnType(): \integer
	{

	}

}

function () {
	$foo = new FooClassForNodeScopeResolverTestingWithoutNamespace();
	assertType('boolean', $foo->misleadingBoolReturnType());
	assertType('integer', $foo->misleadingIntReturnType());
};
