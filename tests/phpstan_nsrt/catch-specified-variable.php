<?php

namespace TryCatchWithSpecifiedVariable;

use function PHPStan\Testing\assertType;

class FooException extends \Exception
{

}

function () {
	/** @var string|null $foo */
	$foo = doFoo();
	if ($foo !== null) {
		return;
	}

	try {
		maybeThrows();
	} catch (FooException $foo) {
		assertType('TryCatchWithSpecifiedVariable\FooException', $foo);
	}
};
