<?php

namespace LoopVariables;

use function PHPStan\Testing\assertType;

function () {
	$foo = null;
	$i = 0;
	$nullableVal = null;
	$falseOrObject = false;
	while (($val = fetch()) && $i++ < 10) {
		assertType('LoopVariables\Foo|LoopVariables\Lorem|null', $foo);
		assertType('int<1, max>|null', $nullableVal);
		assertType('LoopVariables\Foo|false', $falseOrObject);
		$foo = new Foo();
		assertType('LoopVariables\Foo', $foo);

		if ($nullableVal === null) {
			assertType('null', $nullableVal);
			$nullableVal = 1;
		} else {
			$nullableVal *= 10;
			assertType('int<10, max>', $nullableVal);
		}

		if ($falseOrObject === false) {
			$falseOrObject = new Foo();
		}

		if (something()) {
			$foo = new Bar();
			break;
		}
		if (something()) {
			$foo = new Baz();
			return;
		}
		if (something()) {
			$foo = new Lorem();
			continue;
		}

		assertType('LoopVariables\Foo', $foo);

		assertType('LoopVariables\Foo', $falseOrObject);

	}


	assertType('LoopVariables\Bar|LoopVariables\Foo|LoopVariables\Lorem|null', $foo);

	assertType('1|int<10, max>|null', $nullableVal);

	assertType('LoopVariables\Foo|false', $falseOrObject);
};
