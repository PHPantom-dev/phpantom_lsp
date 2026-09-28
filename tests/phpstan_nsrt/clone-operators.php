<?php

namespace CloneOperators;

use function PHPStan\Testing\assertType;

class Foo
{

}

function () {
	$fooObject = new Foo();

	assertType('CloneOperators\Foo', clone $fooObject);
};
