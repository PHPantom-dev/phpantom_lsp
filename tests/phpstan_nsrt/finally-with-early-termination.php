<?php

namespace FinallyNamespace;

use function PHPStan\Testing\assertType;

try {
	$integerOrString = 1;
	$fooOrBarException = null;
	maybeThrows();
	return 1;
} catch (FooException $e) {
	$integerOrString = 1;
	$fooOrBarException = $e;
	throw $e;
} catch (BarException $e) {
	$integerOrString = 'foo';
	$fooOrBarException = $e;
	return $e;
} finally {
	assertType('1|\'foo\'', $integerOrString);
	assertType('FinallyNamespace\BarException|FinallyNamespace\FooException|null', $fooOrBarException);
}
