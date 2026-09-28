<?php

declare(strict_types=1);

namespace Bug14084;

use function PHPStan\Testing\assertType;

/**
 * @param array<string, list<string>> $convert
 */
function example(array $convert): void
{
	foreach ($convert as &$inner) {
		foreach ($inner as &$val) {
			$val = strtoupper($val); // https://github.com/phpstan/phpstan/issues/14083
		}
	}
	assertType('array<string, list<string>>', $convert);
}

/**
 * @param array<string, list<string>> $convert
 */
function example2(array $convert): void
{
	foreach ($convert as &$inner) {
		foreach ($inner as $key => $val) {
			$inner[$key] = strtoupper($val);
		}
	}
	assertType('array<string, list<string>>', $convert);
}

/**
 * @param array<string, list<string>> $convert
 */
function example3(array &$convert): void
{
	foreach ($convert as $outerKey => $inner) {
		foreach ($inner as $key => $val) {
			$convert[$outerKey][$key] = strtoupper($val);
		}
	}
	assertType('array<string, list<string>>', $convert);
}
