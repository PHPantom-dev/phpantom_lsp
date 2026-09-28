<?php

namespace CliGlobals;

use function PHPStan\Testing\assertType;


function f() {
	assertType('*ERROR*', $argc);
	assertType('*ERROR*', $argv);
}

function g($argc, $argv) {
	assertType('mixed', $argc);
	assertType('mixed', $argv);
}

function h() {
	global $argc, $argv;
	assertType('mixed', $argc); // should be int<1, max>
	assertType('mixed', $argv); // should be non-empty-array<int, string>
}

function i() {
	// user created local variable
	$argc = 'hallo';
	$argv = 'welt';

	assertType("'hallo'", $argc);
	assertType("'welt'", $argv);
}
