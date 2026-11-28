<?php

namespace GetDefinedVars;

use function PHPStan\Testing\assertType;

$global = "foo";

assertType('array<string, mixed>', get_defined_vars()); // any variable can exist

function doFoo(int $param) {
	$local = "foo";
	assertType('array{param: int, local: \'foo\'}', get_defined_vars());
	assertType('array{\'param\', \'local\'}', array_keys(get_defined_vars())); // SKIP: the runner's own `$__phpantom_assert_N` variable from the line above is in scope here
}

function doBar(int $param) {
	global $global;
	$local = "foo";
	assertType('array{param: int, global: \'foo\', local: \'foo\'}', get_defined_vars()); // PHPStan says `global: mixed`; PHPantom reads a `global` from the top-level scope
	assertType('array{\'param\', \'global\', \'local\'}', array_keys(get_defined_vars())); // SKIP: the runner's own `$__phpantom_assert_N` variable from the line above is in scope here
}

function doConditional(int $param) {
	$local = "foo";
	if(true) {
		$conditional = "bar";
		assertType('array{param: int, local: \'foo\', conditional: \'bar\'}', get_defined_vars());
	} else {
		$other = "baz";
		assertType('array{param: int, local: \'foo\', other: \'baz\'}', get_defined_vars());
	}
	assertType('array{param: int, local: \'foo\', conditional: \'bar\'}', get_defined_vars()); // SKIP: the runner's own `$__phpantom_assert_N` variable from the branch above is in scope here
}

function doRandom(int $param) {
	$local = "foo";
	if(rand(0, 1)) {
		$random1 = "bar";
		assertType('array{param: int, local: \'foo\', random1: \'bar\'}', get_defined_vars());
	} else {
		$random2 = "baz";
		assertType('array{param: int, local: \'foo\', random2: \'baz\'}', get_defined_vars());
	}
	assertType('array{param: int, local: \'foo\', random1?: \'bar\', random2?: \'baz\'}', get_defined_vars()); // SKIP: the runner's own `$__phpantom_assert_N` variable from the branches above is in scope here; PHPStan lists `random2` first, PHP lists them in source order
}

class A
{
	public function test(): void
	{
		$local = 'foo';

		assertType('array{local: \'foo\'}', get_defined_vars());
	}
}
