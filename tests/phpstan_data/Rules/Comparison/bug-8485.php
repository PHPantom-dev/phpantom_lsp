<?php // lint >= 8.1

namespace Bug8485;

use function PHPStan\Testing\assertType;

enum E {
	case c;
}

enum F {
	case c;
}

function shouldError():void {
	$e = E::c;
	$f = F::c;

	if ($e === E::c) {
	}
	if ($e == E::c) {
	}

	if ($f === $e) {
	}
	if ($f == $e) {
	}

	if ($f === E::c) {
	}
	if ($f == E::c) {
	}
}

function allGood(E $e, F $f):void {
	if ($f === $e) {
	}
	if ($f == $e) {
	}

	if ($f === E::c) {
	}
	if ($f == E::c) {
	}
}

enum FooEnum
{
	case A;
	case B;
	case C;
}
function dooFoo(FooEnum $s):void {
	if ($s === FooEnum::A) {
	} elseif ($s === FooEnum::B) {
	} elseif ($s === FooEnum::C) {
	}

	if ($s === FooEnum::A) {
	} elseif ($s === FooEnum::B) {
	} else {
		assertType('Bug8485\FooEnum::C', $s);
	}

	if ($s === FooEnum::A) {
	} elseif ($s === FooEnum::B) {
	} elseif ($s === FooEnum::C) {
	} else {
		assertType('*NEVER*', $s);
	}

	if ($s === FooEnum::A) {
	} elseif ($s === FooEnum::B) {
	} elseif ($s === FooEnum::C) {
	} elseif (rand(0, 1)) {
	}

}
