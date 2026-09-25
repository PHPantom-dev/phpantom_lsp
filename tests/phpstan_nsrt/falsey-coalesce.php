<?php

namespace FalseyCoalesce;

use PHPStan\TrinaryLogic;
use function PHPStan\Testing\assertType;
use function PHPStan\Testing\assertVariableCertainty;

interface TipPluginInterface
{

	/**
	 * Used for returning values by key.
	 *
	 * @return string
	 *   Value of the key.
	 * @var string
	 *   Key of the value.
	 *
	 */
	public function get($key);

}

abstract class TipPluginBase implements TipPluginInterface
{

	public function getLocation(): void
	{
		$location = $this->get('position');
		assertVariableCertainty(TrinaryLogic::createYes(), $location);

			$location ?? '';
		assertVariableCertainty(TrinaryLogic::createYes(), $location);
	}

}

function maybeTrueVarAssign():void {
	if (rand(0,1)) {
		$a = true;
	}
	$x = $a ?? ($y=1) ?? 1;

	assertVariableCertainty(TrinaryLogic::createYes(), $x);
	assertVariableCertainty(TrinaryLogic::createMaybe(), $a);
	assertType('true', $a);
	assertVariableCertainty(TrinaryLogic::createMaybe(), $y);
	assertType('1', $y);
}

function nullableVarAssign():void {
	if (rand(0,1)) {
		$a = true;
	} else {
		$a = null;
	}
	$x = $a ?? ($y=1) ?? 1;

	assertVariableCertainty(TrinaryLogic::createYes(), $x);
	assertVariableCertainty(TrinaryLogic::createYes(), $a);
	assertType('true|null', $a);
	assertVariableCertainty(TrinaryLogic::createMaybe(), $y);
	assertType('1', $y);
}

function maybeNullableVarAssign():void {
	if (rand(0,1)) {
		$a = null;
	}
	$x = $a ?? ($y=1) ?? 1;

	assertVariableCertainty(TrinaryLogic::createYes(), $x);
	assertType('1', $x);
	assertVariableCertainty(TrinaryLogic::createMaybe(), $a);
	assertType('null', $a);
	assertVariableCertainty(TrinaryLogic::createMaybe(), $y);
	assertType('1', $y);
}

function notExistsAssign():void {
	$x = $a ?? ($y=1) ?? 1;

	assertVariableCertainty(TrinaryLogic::createYes(), $x);
	assertType('1', $x);
	assertVariableCertainty(TrinaryLogic::createNo(), $a);
	assertType('*ERROR*', $a);
	assertVariableCertainty(TrinaryLogic::createMaybe(), $y);
	assertType('1', $y);
}

function nullableVarExpr():void {
	if (rand(0,1)) {
		$a = true;
	} else {
		$a = null;
	}
	$a ?? ($y=1) ?? 1;

	assertVariableCertainty(TrinaryLogic::createYes(), $a);
	assertType('true|null', $a);
	assertVariableCertainty(TrinaryLogic::createMaybe(), $y);
	assertType('1', $y);
}

function maybeNullableVarExpr():void {
	if (rand(0,1)) {
		$a = null;
	}
	$a ?? ($y=1) ?? 1;

	assertVariableCertainty(TrinaryLogic::createMaybe(), $a);
	assertType('null', $a);
	assertVariableCertainty(TrinaryLogic::createMaybe(), $y);
	assertType('1', $y);
}

function notExistsExpr():void {
	$a ?? ($y=1) ?? 1;

	assertVariableCertainty(TrinaryLogic::createNo(), $a);
	assertType('*ERROR*', $a);
	assertVariableCertainty(TrinaryLogic::createMaybe(), $y);
	assertType('1', $y);
}
