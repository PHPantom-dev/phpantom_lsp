<?php

namespace FinallyNamespace;

use function PHPStan\Testing\assertType;

class FooException extends \Exception
{

}

class BarException extends \Exception
{

}

function () {
	try {
		$integerOrString = 1;
		$fooOrBarException = null;
		maybeThrows();
	} catch (FooException $e) {
		$integerOrString = 1;
		$fooOrBarException = $e;
	} catch (BarException $e) {
		$integerOrString = 'foo';
		$fooOrBarException = $e;
	} finally {
		assertType('1|\'foo\'', $integerOrString);
		assertType('FinallyNamespace\BarException|FinallyNamespace\FooException|null', $fooOrBarException);
	}
};
