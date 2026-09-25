<?php // lint >= 8.0

namespace FalsyIsset;

use function PHPStan\Testing\assertType;
use function PHPStan\Testing\assertVariableCertainty;
use PHPStan\TrinaryLogic;

function doFoo():mixed {
	return 1;
}

function maybeMixedVariable(): void
{
	if (rand(0,1)) {
		$a = doFoo();
	}

	if (isset($a)) {
	} else {
		assertType("null", $a);
	}
}

function maybeNullableVariable(): void
{
	if (rand(0,1)) {
		$a = 'hello';

		if (rand(0,2)) {
			$a = null;
		}
	}

	if (isset($a)) {
		assertType("'hello'", $a);
	} else {
		assertType("null", $a);
	}
}

function subtractedMixedIsset(mixed $m): void
{
	if ($m === null) {
		return;
	}

	if (isset($m)) {
	} else {
	}
}

function mixedIsset(mixed $m): void
{
	if (isset($m)) {
	} else {
		assertType("null", $m);
	}
}

function stdclassIsset(?\stdClass $m): void
{
	if (isset($m)) {
		assertType("stdClass", $m);
	} else {
		assertType("null", $m);
	}
}

function nullableVariable(?string $a): void
{
	if (isset($a)) {
		assertType("string", $a);
	} else {
		assertType("null", $a);
	}
}

function nullableUnionVariable(null|string|int $a): void
{
	if (isset($a)) {
		assertType("int|string", $a);
	} else {
		assertType("null", $a);
	}
}

function render(?int $noteListLimit, int $count): void
{
	$showAllLink = $noteListLimit !== null && $count > $noteListLimit;
	if ($showAllLink) {
		assertType('int', $noteListLimit); // SKIP: a condition stored in a variable loses its narrowing
	}
}
