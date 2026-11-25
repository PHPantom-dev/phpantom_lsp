<?php

namespace ThrowPoints\Assign;

use PHPStan\TrinaryLogic;
use stdClass;
use function PHPStan\Testing\assertType;
use function PHPStan\Testing\assertVariableCertainty;
use function ThrowPoints\Helpers\doesntThrow;
use function ThrowPoints\Helpers\maybeThrows;

class Foo
{

	/** @var bool */
	public static $bar = true;

}

function () {
	try {
		$foo = doesntThrow();
	} finally {
		assertVariableCertainty(TrinaryLogic::createYes(), $foo);
	}
};

function () {
	try {
		$foo = maybeThrows();
	} finally {
		assertVariableCertainty(TrinaryLogic::createMaybe(), $foo);
	}
};

function () {
	try {
		$foo[0] = doesntThrow();
	} finally {
		assertVariableCertainty(TrinaryLogic::createYes(), $foo);
	}
};

function () {
	try {
		$foo[0] = maybeThrows();
	} finally {
		assertVariableCertainty(TrinaryLogic::createMaybe(), $foo);
	}
};

function () {
	try {
		$foo[doesntThrow()] = 0;
	} finally {
		assertVariableCertainty(TrinaryLogic::createYes(), $foo);
	}
};

function () {
	try {
		$foo[maybeThrows()] = 0;
	} finally {
		assertVariableCertainty(TrinaryLogic::createMaybe(), $foo);
	}
};

function () {
	try {
		$foo = new stdClass();
		$foo->bar = false;
		$foo->bar = (doesntThrow() || true);
	} finally {
	}
};

function () {
	try {
		$foo = new stdClass();
		$foo->bar = false;
		$foo->bar = (maybeThrows() || true);
	} finally {
		assertType('bool', $foo->bar);
	}
};

function () {
	try {
		$obj = new stdClass();
		$obj->{doesntThrow()} = ($foo = 1);
	} finally {
		assertVariableCertainty(TrinaryLogic::createYes(), $foo);
	}
};

function () {
	try {
		$obj = new stdClass();
		$obj->{maybeThrows()} = ($foo = 1);
	} finally {
		assertVariableCertainty(TrinaryLogic::createMaybe(), $foo);
	}
};

function () {
	try {
		Foo::$bar = false;
		Foo::$bar = (doesntThrow() || true);
	} finally {
	}
};

function () {
	try {
		Foo::$bar = false;
		Foo::$bar = (maybeThrows() || true);
	} finally {
		assertType('bool', Foo::$bar);
	}
};

function () {
	try {
		Foo::${doesntThrow()} = ($foo = 1);
	} finally {
		assertVariableCertainty(TrinaryLogic::createYes(), $foo);
	}
};

function () {
	try {
		Foo::${maybeThrows()} = ($foo = 1);
	} finally {
		assertVariableCertainty(TrinaryLogic::createMaybe(), $foo);
	}
};

function () {
	try {
		[$foo] = doesntThrow();
	} finally {
		assertVariableCertainty(TrinaryLogic::createYes(), $foo);
	}
};

function () {
	try {
		[$foo] = maybeThrows();
	} finally {
		assertVariableCertainty(TrinaryLogic::createMaybe(), $foo);
	}
};

function () {
	try {
		[$foo[doesntThrow()]] = 1;
	} finally {
		assertVariableCertainty(TrinaryLogic::createYes(), $foo);
	}
};

function () {
	try {
		[$foo[maybeThrows()]] = 1;
	} finally {
		assertVariableCertainty(TrinaryLogic::createMaybe(), $foo);
	}
};
