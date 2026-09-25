<?php

namespace Analyser\Bug2577;

use function PHPStan\Testing\assertType;

class A {}
class A1 extends A {}
class A2 extends A {}

/**
 * @template T of A
 *
 * @param \Closure():T $t1
 * @param T $t2
 * @return T
 */
function echoOneOrOther(\Closure $t1, A $t2) {
	echo get_class($t1());
	echo get_class($t2);
	throw new \Exception();
}

function test(): void {
	$result = echoOneOrOther(
		function () : A1 {
			return new A1;
		},
		new A2
	);
	assertType('Analyser\Bug2577\A1|Analyser\Bug2577\A2', $result);
}
