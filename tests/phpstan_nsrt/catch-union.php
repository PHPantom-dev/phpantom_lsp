<?php

namespace CatchUnion;

use function PHPStan\Testing\assertType;

class FooException extends \Exception
{

}

class BarException extends \Exception
{

}

function () {
	try {
		maybeThrows();
	} catch (FooException | BarException $e) {
		assertType('CatchUnion\BarException|CatchUnion\FooException', $e);
	}
};
