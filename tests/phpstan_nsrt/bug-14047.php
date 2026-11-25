<?php

namespace Bug14047;

use function is_numeric;
use function PHPStan\Testing\assertType;

function test_strings(string $a, string $b): void
{
	if (strtolower($a) !== $b) {
		assertType('string', $a);
		assertType('string', $b);
	}

	if ($a !== '' && strtolower($a) == $b) {
		assertType('non-empty-string', $a);
	}

	if ($a !== '' && strtolower($a) === $b) {
		assertType('non-empty-string', $a);
	}

	if ($a !== '' && strtolower($a) == $b && $b !== '0') {
		assertType('non-empty-string', $a);
	}

	if ($a !== '' && strtolower($a) === $a) {
	} elseif ($a !== '' && strtoupper($a) === $a) {
	}

	if ($a !== '') {
		if (strtolower($a) === $a) {
		}
	}

	if (strtolower($b) === $b && $b !== '') {
	} elseif (strtoupper($b) === $b && $b !== '') {
	}

	if ($b !== '' && is_numeric($b)) {
	}

	if (strtolower($b) === $b && $b !== '' && is_numeric($b)) {
	}

	assertType('string', $b);
	if (is_numeric($b)) {
		assertType('numeric-string', $b);
		if (strtolower($b) === $b) {
			if ($b !== '') {
			}
		}
		if ($b !== '') {
			assertType('numeric-string', $b);
		}
	}
}
