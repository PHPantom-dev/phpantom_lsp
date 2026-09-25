<?php

namespace DependentVariableCertainty;

use PHPStan\TrinaryLogic;
use function PHPStan\Testing\assertType;
use function PHPStan\Testing\assertVariableCertainty;

function returnsBool($val): bool {
	return (bool) rand(0, 1);
}

function ($b, $c): void {
	if (returnsBool($b)) {
		$foo = 'bla';
	}

	assertVariableCertainty(TrinaryLogic::createMaybe(), $foo);

	if (returnsBool($b)) {
		assertVariableCertainty(TrinaryLogic::createYes(), $foo);
	} else {
		assertVariableCertainty(TrinaryLogic::createNo(), $foo);
	}

	if (returnsBool($b)) {

	}

	if (returnsBool($b)) {
		assertVariableCertainty(TrinaryLogic::createYes(), $foo);
	} else {
		assertVariableCertainty(TrinaryLogic::createNo(), $foo);
	}

	if (returnsBool($b)) {
		$d = true;
	}

	if (returnsBool($b)) {
		assertVariableCertainty(TrinaryLogic::createYes(), $foo);
	} else {
		assertVariableCertainty(TrinaryLogic::createNo(), $foo);
	}

	if (returnsBool($c)) {
		$bar = 'ble';
	}

	assertVariableCertainty(TrinaryLogic::createMaybe(), $foo);

	if (returnsBool($b)) {
		assertVariableCertainty(TrinaryLogic::createYes(), $foo);
	} else {
		assertVariableCertainty(TrinaryLogic::createNo(), $foo);
	}

	assertVariableCertainty(TrinaryLogic::createMaybe(), $foo);
	assertVariableCertainty(TrinaryLogic::createMaybe(), $bar);

	if (returnsBool($c)) {
		assertVariableCertainty(TrinaryLogic::createMaybe(), $foo);
		assertVariableCertainty(TrinaryLogic::createYes(), $bar);
	} else {
		assertVariableCertainty(TrinaryLogic::createMaybe(), $foo);
		assertVariableCertainty(TrinaryLogic::createNo(), $bar);
	}
};

function (bool $b): void {
	if (returnsBool($b)) {
		$foo = 'bla';
	}

	$b = true;

	assertVariableCertainty(TrinaryLogic::createMaybe(), $foo);

	if (returnsBool($b)) {
		assertVariableCertainty(TrinaryLogic::createMaybe(), $foo);
	} else {
		assertVariableCertainty(TrinaryLogic::createMaybe(), $foo);
	}
};

function (bool $b): void {
	if (returnsBool($b)) {
		$foo = 'bla';
	}

	$foo = 'ble';

	assertVariableCertainty(TrinaryLogic::createYes(), $foo);

	if (returnsBool($b)) {
		assertVariableCertainty(TrinaryLogic::createYes(), $foo);
	} else {
		assertVariableCertainty(TrinaryLogic::createYes(), $foo);
	}
};

function (bool $a, bool $b) {
	if (returnsBool($a)) {
		$lorem = 'test';
	}

	if (returnsBool($a)) {
		assertVariableCertainty(TrinaryLogic::createYes(), $lorem);
	}

	while (returnsBool($b)) {
		$foo = 'foo';
		if (rand(0, 1)) {
			break;
		}
	}

	if (returnsBool($a)) {
		assertVariableCertainty(TrinaryLogic::createYes(), $lorem);
	}

	assertVariableCertainty(TrinaryLogic::createMaybe(), $foo);
	if (returnsBool($b)) {
		assertVariableCertainty(TrinaryLogic::createYes(), $foo);
	}

	if (returnsBool($a)) {
		assertVariableCertainty(TrinaryLogic::createYes(), $lorem);
	}
};

function (bool $a, bool $b) {
	if (returnsBool($a)) {
		$lorem = 'test';
	}

	if (returnsBool($a)) {
		assertVariableCertainty(TrinaryLogic::createYes(), $lorem);
	}

	$i = 0;
	while (returnsBool($b)) {
		$foo = 'foo';
		$i++;
		if (rand(0, 1)) {
			break;
		}
	}

	if (returnsBool($a)) {
		assertVariableCertainty(TrinaryLogic::createYes(), $lorem);
	}

	assertVariableCertainty(TrinaryLogic::createMaybe(), $foo);
	if (returnsBool($b)) {
		assertVariableCertainty(TrinaryLogic::createYes(), $foo);
	}

	if (returnsBool($a)) {
		assertVariableCertainty(TrinaryLogic::createYes(), $lorem);
	}
};

function (bool $a, bool $b): void
{
	if (returnsBool($a)) {
		$lorem = 'test';
	}

	if (returnsBool($a)) {
		assertVariableCertainty(TrinaryLogic::createYes(), $lorem);
	}

	unset($b);

	if (returnsBool($a)) {
		assertVariableCertainty(TrinaryLogic::createYes(), $lorem);
	}
};

function (bool $a, bool $b): void
{
	if (returnsBool($a)) {
		$lorem = 'test';
	}

	if (returnsBool($a)) {
		assertVariableCertainty(TrinaryLogic::createYes(), $lorem);
	}

	unset($lorem);

	assertVariableCertainty(TrinaryLogic::createNo(), $lorem);

	if (returnsBool($a)) {
		assertVariableCertainty(TrinaryLogic::createNo(), $lorem);
	}
};

function (bool $is_valid_a): void {
	if (returnsBool($is_valid_a)) {
		$a = new \stdClass();
	} else {
		$a = null;
	}

	assertType('stdClass|null', $a);

	if (returnsBool($is_valid_a)) {
		assertType('stdClass', $a);
	} else {
		assertType('null', $a);
	}
};

function (bool $a, bool $b): void
{
	if (returnsBool($a)) {
		$lorem = 'test';
	}

	if (returnsBool($a)) {
		assertVariableCertainty(TrinaryLogic::createYes(), $lorem);
	}

	unset($a);

	assertVariableCertainty(TrinaryLogic::createMaybe(), $lorem);

	if (returnsBool($a)) {
		assertVariableCertainty(TrinaryLogic::createMaybe(), $lorem);
	}
};
