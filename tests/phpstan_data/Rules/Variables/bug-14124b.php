<?php

namespace Bug14124b;

use function PHPStan\Testing\assertType;

/**
 * @param array<string, list<list<string>>> $convert
 * @param-out array<string, list<list<string>>> $convert
 */
function example3a(array &$convert): void
{
	foreach ($convert as &$inner) {
		foreach ($inner as &$val) {
			foreach ($val as &$val2) {
				$val2 = strtoupper($val2);
			}
		}
	}
	assertType('array<string, list<list<string>>>', $convert);
}

/**
 * @param array<string, list<list<string>>> $convert
 * @param-out array<string, list<list<string>>> $convert
 */
function example3b(array &$convert): void
{
	foreach ($convert as $outerKey => $inner) {
		foreach ($inner as $key => $val) {
			foreach ($val as $key2 => $val2) {
				$convert[$outerKey][$key][$key2] = strtoupper($val);
			}
		}
	}
	assertType('array<string, list<list<string>>>', $convert);
}
