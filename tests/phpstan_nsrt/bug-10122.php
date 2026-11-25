<?php

namespace Bug10122;

use function PHPStan\Testing\assertType;

function doFoo():void
{
	function(string $s) {
		assertType('(float|int|string)', ++$s);
	};
	function(string $s) {
		assertType('(float|int|string)', --$s);
	};
	function(string $s) {
		assertType('string', $s++);
		assertType('(float|int|string)', $s);
	};
	function(string $s) {
		assertType('string', $s--);
		assertType('(float|int|string)', $s);
	};

	function(float $f) {
		assertType('float', ++$f);
	};
	function(float $f) {
		assertType('float', --$f);
	};
	function(float $f) {
		assertType('float', $f++);
	};
	function(float $f) {
		assertType('float', $f--);
	};
}

/** @param numeric-string $ns */
function doNumericString(string $ns) {
	function() use ($ns) {
		assertType('(float|int)', ++$ns);
	};
	function() use ($ns) {
		assertType('(float|int)', --$ns);
	};
	function() use ($ns) {
		assertType('numeric-string', $ns++);
		assertType('(float|int)', $ns);
	};
	function() use ($ns) {
		assertType('numeric-string', $ns--);
		assertType('(float|int)', $ns);
	};
}
