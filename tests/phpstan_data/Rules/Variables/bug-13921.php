<?php

namespace Bug13921;

use SimpleXMLElement;
use function PHPStan\Testing\assertType;

/** @param list<array<?string>> $x */
function foo(array $x): void {
	var_dump($x[0]['bar'] ?? null);
	assertType("list<array<string|null>>", $x);
	var_dump($x[0] ?? null);
}

/** @param non-empty-list<array<?string>> $x */
function nonEmptyFoo(array $x): void {
	var_dump($x[0]['bar'] ?? null);
	assertType("non-empty-list<array<string|null>>", $x);
	var_dump($x[0] ?? null);
}

/** @param list<array<?string>> $x */
function bar(array $x): void {
	var_dump($x[0] ?? null);
	assertType("list<array<string|null>>", $x);
	var_dump($x[0]['bar'] ?? null);
}

/** @param list<array<?string>> $x */
function baz(array $x): void {
	var_dump($x[1] ?? null);
	assertType("list<array<string|null>>", $x);
	var_dump($x[0]['bar'] ?? null);
}

/** @param list<array<?string>> $x */
function boo(array $x): void {
	var_dump($x[0]['bar'] ?? null);
	assertType("list<array<string|null>>", $x);
	var_dump($x[1] ?? null);
}

function doBar(array $array)
{
	if (isset($array['foo'])) {
	}
}

/** @param list<SimpleXMLElement> $x */
function sooSimpleElement(array $x): void {
	var_dump($x[0]['bar'] ?? null);
	assertType("list<SimpleXMLElement>", $x);
	var_dump($x[0] ?? null);
}
