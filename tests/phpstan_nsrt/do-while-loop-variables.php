<?php

namespace LoopVariables;

use function PHPStan\Testing\assertType;

function () {
	$foo = null;
	$i = 0;
	$nullableVal = null;
	$falseOrObject = false;
	$anotherFalseOrObject = false;
	do {
		assertType('LoopVariables\Foo|LoopVariables\Lorem|null', $foo);
		assertType('int<1, max>|null', $nullableVal);
		assertType('LoopVariables\Foo|false', $falseOrObject);
		assertType('LoopVariables\Foo|false', $anotherFalseOrObject);
		$foo = new Foo();
		assertType('LoopVariables\Foo', $foo);

		if ($nullableVal === null) {
			assertType('null', $nullableVal);
			$nullableVal = 1;
		} else {
			$nullableVal *= 10;
			assertType('int<10, max>', $nullableVal);
		}

		if ($anotherFalseOrObject === false) {
			$anotherFalseOrObject = new Foo();
		}

		if (doFoo()) {
			break;
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

		$i++;

		assertType('LoopVariables\Foo', $foo);


		assertType('LoopVariables\Foo', $falseOrObject);

		assertType('LoopVariables\Foo', $anotherFalseOrObject);
	} while (doFoo() && $i++ < 10);

	assertType('LoopVariables\Bar|LoopVariables\Foo|LoopVariables\Lorem', $foo);


	assertType('1|int<10, max>', $nullableVal);

	assertType('LoopVariables\Foo|false', $falseOrObject);

	assertType('LoopVariables\Foo', $anotherFalseOrObject);
};
