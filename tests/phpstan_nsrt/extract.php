<?php

namespace Extract;

use PHPStan\TrinaryLogic;
use function PHPStan\Testing\assertType;
use function PHPStan\Testing\assertVariableCertainty;

function doUntyped(array $vars): void
{
	extract($vars);

	assertVariableCertainty(TrinaryLogic::createMaybe(), $a);
}

/**
 * @param array{a: string, b?: int} $vars
 */
function doTyped(array $vars): void
{
	extract($vars);

	assertVariableCertainty(TrinaryLogic::createYes(), $a);
	assertType('string', $a);

	assertVariableCertainty(TrinaryLogic::createMaybe(), $b);
	assertType('int', $b);

	assertVariableCertainty(TrinaryLogic::createNo(), $none);
}

/**
 * @param array{a: string, b?: int}|array{a: int, b: int, c: string} $vars
 */
function doTyped2(array $vars): void
{
	extract($vars);

	assertVariableCertainty(TrinaryLogic::createYes(), $a);
	assertType('int|string', $a);

	assertVariableCertainty(TrinaryLogic::createMaybe(), $b);
	assertType('int', $b);

	assertVariableCertainty(TrinaryLogic::createMaybe(), $c);
	assertType('string', $c);

	assertVariableCertainty(TrinaryLogic::createNo(), $none);
}

/**
 * @param array{a: string, b?: int}|array{a: int, b: int, c: string}|array{c: int} $vars
 */
function doTyped3(array $vars): void
{
	extract($vars);

	assertVariableCertainty(TrinaryLogic::createMaybe(), $a);
	assertType('int|string', $a);

	assertVariableCertainty(TrinaryLogic::createMaybe(), $b);
	assertType('int', $b);

	assertVariableCertainty(TrinaryLogic::createMaybe(), $c);
	assertType('int|string', $c);

	assertVariableCertainty(TrinaryLogic::createNo(), $none);
}

function doTyped4(): void
{
	extract(['foo' => 42]);

	assertVariableCertainty(TrinaryLogic::createYes(), $foo);
	assertType('42', $foo);
}


function doTyped5(): void
{
	$foo = ['foo' => 42];
	extract($foo);

	assertVariableCertainty(TrinaryLogic::createYes(), $foo);
	assertType('42', $foo);
}
