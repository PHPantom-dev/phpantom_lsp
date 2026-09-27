<?php

namespace Bug14124;

use function PHPStan\Testing\assertType;

/**
 * @param array<string, list<string>> $convert
 * @param-out array<string, list<string>> $convert
 */
function example3a(array &$convert): void
{
	foreach ($convert as &$inner) {
		foreach ($inner as &$val) {
			$val = strtoupper($val);
		}
	}
	assertType('array<string, list<string>>', $convert);
}

/**
 * @param array<string, list<string>> $convert
 * @param-out array<string, list<string>> $convert
 */
function example3b(array &$convert): void
{
	foreach ($convert as $outerKey => $inner) {
		foreach ($inner as $key => $val) {
			$convert[$outerKey][$key] = strtoupper($val);
		}
	}
	assertType('array<string, list<string>>', $convert);
}
