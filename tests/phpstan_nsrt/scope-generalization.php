<?php

namespace ScopeGeneralization;

use function PHPStan\Testing\assertType;

function loopAddsAccessory(): void
{
	/** @var array<string> */
	$foo = [];
	for ($i = 0; $i < 3; $i++) {
		array_push($foo, 'foo');
	}
	assertType('non-empty-array<string>', $foo);
}

function loopRemovesAccessory(): void
{
	/** @var non-empty-array<string> */
	$foo = [];
	for ($i = 0; $i < 3; $i++) {
		array_pop($foo);
	}
	assertType('array<string>', $foo);
}

function closureRemovesAccessoryOfReferenceParameter(): void
{
	/** @var non-empty-array<string> */
	$foo = [];
	static function () use (&$foo) {
		assertType('array<string>', $foo);
		array_pop($foo);
	};
}
