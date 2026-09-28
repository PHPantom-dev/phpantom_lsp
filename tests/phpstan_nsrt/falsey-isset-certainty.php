<?php // lint >= 8.0

namespace FalseyIssetCertainty;

use function PHPStan\Testing\assertType;
use function PHPStan\Testing\assertVariableCertainty;
use PHPStan\TrinaryLogic;

function getFoo():mixed {
	return 1;
}

function falseyIssetArrayDimFetchOnProperty(): void
{
	$a = new \stdClass();
	$a->bar = null;
	if (rand() % 3) {
		$a->bar = 'hello';
	}

	assertVariableCertainty(TrinaryLogic::createYes(), $a);
	if (isset($a->bar)) {
		assertVariableCertainty(TrinaryLogic::createYes(), $a);
	} else {
		assertVariableCertainty(TrinaryLogic::createYes(), $a);
	}

	assertVariableCertainty(TrinaryLogic::createYes(), $a);
}

function falseyIssetUncertainArrayDimFetchOnProperty(): void
{
	if (rand() % 2) {
		$a = new \stdClass();
		$a->bar = null;
		$a = ['bar' => null];
		if (rand() % 3) {
			$a->bar = 'hello';
		}
	}

	assertVariableCertainty(TrinaryLogic::createMaybe(), $a);
	if (isset($a->bar)) {
		assertVariableCertainty(TrinaryLogic::createYes(), $a);
	} else {
		assertVariableCertainty(TrinaryLogic::createMaybe(), $a);
	}

	assertVariableCertainty(TrinaryLogic::createMaybe(), $a);
}

function falseyIssetUncertainPropertyFetch(): void
{
	if (rand() % 2) {
		$a = new \stdClass();
		if (rand() % 3) {
			$a->x = 'hello';
		}
	}

	assertVariableCertainty(TrinaryLogic::createMaybe(), $a);
	if (isset($a->x)) {
		assertVariableCertainty(TrinaryLogic::createYes(), $a);
	} else {
		assertVariableCertainty(TrinaryLogic::createMaybe(), $a);
	}

	assertVariableCertainty(TrinaryLogic::createMaybe(), $a);
}

function falseyIssetArrayDimFetch(): void
{
	$a = ['bar' => null];
	if (rand() % 3) {
		$a = ['bar' => 'hello'];
	}

	assertVariableCertainty(TrinaryLogic::createYes(), $a);
	if (isset($a['bar'])) {
		assertVariableCertainty(TrinaryLogic::createYes(), $a);
	} else {
		assertVariableCertainty(TrinaryLogic::createYes(), $a);
	}

	assertVariableCertainty(TrinaryLogic::createYes(), $a);
}

function falseyIssetUncertainArrayDimFetch(): void
{
	if (rand() % 2) {
		$a = ['bar' => null];
		if (rand() % 3) {
			$a = ['bar' => 'hello'];
		}
	}

	assertVariableCertainty(TrinaryLogic::createMaybe(), $a);
	if (isset($a['bar'])) {
		assertVariableCertainty(TrinaryLogic::createYes(), $a);
	} else {
		assertVariableCertainty(TrinaryLogic::createMaybe(), $a);
	}

	assertVariableCertainty(TrinaryLogic::createMaybe(), $a);
}

function falseyIssetVariable(): void
{
	if (rand() % 2) {
		$a = 'bar';
	}

	assertVariableCertainty(TrinaryLogic::createMaybe(), $a);
	if (isset($a)) {
		assertVariableCertainty(TrinaryLogic::createYes(), $a);
	} else {
		assertVariableCertainty(TrinaryLogic::createNo(), $a);
	}

	assertVariableCertainty(TrinaryLogic::createMaybe(), $a);
}

function nullableVariable(): void
{
	$a = 'bar';
	if (rand() % 2) {
		$a = null;
	}

	assertVariableCertainty(TrinaryLogic::createYes(), $a);
	if (isset($a)) {
		assertVariableCertainty(TrinaryLogic::createYes(), $a);
	} else {
		assertVariableCertainty(TrinaryLogic::createYes(), $a);
	}

	assertVariableCertainty(TrinaryLogic::createYes(), $a);
}

function nonNullableVariable(): void
{
	$a = 'bar';

	assertVariableCertainty(TrinaryLogic::createYes(), $a);
	if (isset($a)) {
		assertVariableCertainty(TrinaryLogic::createYes(), $a);
	} else {
		assertVariableCertainty(TrinaryLogic::createNo(), $a);
	}

	assertVariableCertainty(TrinaryLogic::createYes(), $a);
}

function nonNullableVariableUnset(): void
{
	$a = 'bar';

	assertVariableCertainty(TrinaryLogic::createYes(), $a);
	if (isset($a)) {
		assertVariableCertainty(TrinaryLogic::createYes(), $a);
		unset($a);
		assertVariableCertainty(TrinaryLogic::createNo(), $a);
	} else {
		assertVariableCertainty(TrinaryLogic::createNo(), $a);
	}

	assertVariableCertainty(TrinaryLogic::createNo(), $a);
}

function falseyIssetNullableVariable(): void
{
	if (rand() % 2) {
		$a = 'bar';
		if (rand() % 3) {
			$a = null;
		}
	}

	assertVariableCertainty(TrinaryLogic::createMaybe(), $a);
	if (isset($a)) {
		assertVariableCertainty(TrinaryLogic::createYes(), $a);
	} else {
		assertVariableCertainty(TrinaryLogic::createMaybe(), $a);
	}

	assertVariableCertainty(TrinaryLogic::createMaybe(), $a);
}

function falseyMixedIssetVariable(): void
{
	if (rand() % 2) {
		$a = getFoo();
	}

	assertVariableCertainty(TrinaryLogic::createMaybe(), $a);
	if (isset($a)) {
		assertVariableCertainty(TrinaryLogic::createYes(), $a);
	} else {
		assertVariableCertainty(TrinaryLogic::createMaybe(), $a);
	}

	assertVariableCertainty(TrinaryLogic::createMaybe(), $a);
}

function falseySubtractedMixedIssetVariable(): void
{
	if (rand() % 2) {
		$a = getFoo();
		if ($a === null) {
			return;
		}
	}

	assertVariableCertainty(TrinaryLogic::createMaybe(), $a);
	if (isset($a)) {
		assertVariableCertainty(TrinaryLogic::createYes(), $a);
	} else {
		assertVariableCertainty(TrinaryLogic::createNo(), $a);
	}

	assertVariableCertainty(TrinaryLogic::createMaybe(), $a);
}

function falseyIssetWithAssignment(): void
{
	if (rand() % 2) {
		$x = ['x' => 1];
	}

	if (isset($x[$z = getFoo()])) {
		assertVariableCertainty(TrinaryLogic::createYes(), $z);
		assertVariableCertainty(TrinaryLogic::createYes(), $x);

	} else {
		assertVariableCertainty(TrinaryLogic::createYes(), $z);
		assertVariableCertainty(TrinaryLogic::createMaybe(), $x);
	}

	assertVariableCertainty(TrinaryLogic::createYes(), $z);
	assertVariableCertainty(TrinaryLogic::createMaybe(), $x);
}

function justIsset(): void
{
	if (isset($foo)) {
		assertVariableCertainty(TrinaryLogic::createNo(), $foo);
	}
}

function maybeIsset(): void
{
	if (rand() % 2) {
		$foo = 1;
	}
	assertVariableCertainty(TrinaryLogic::createMaybe(), $foo);
	if (isset($foo)) {
		assertVariableCertainty(TrinaryLogic::createYes(), $foo);
		assertType('1', $foo);
	}
}

function isStringNarrowsMaybeCertainty(int $i, string $s): void
{
	if (rand(0, 1)) {
		$a = rand(0,1) ? $i : $s;
	}

	if (is_string($a)) {
		assertVariableCertainty(TrinaryLogic::createYes(), $a);
		echo $a;
	}
}

function parseVariableSymbolFromXmlNode(SimpleXMLElement $transactionXmlElement): string
{
	if (
		isset($transactionXmlElement->invoice_number)
		&& preg_match('~^\d+/\d+\-0*(\d+)$~', (string) $transactionXmlElement->invoice_number, $matches) === 1
	) {
		assertVariableCertainty(TrinaryLogic::createYes(), $matches);
	}
}
