<?php

namespace NarrowBoolCast;

use function PHPStan\Testing\assertType;

/** @param array<mixed> $arr */
function doFoo(string $x, array $arr): void {
	if ((bool) strlen($x)) {
		assertType('string', $x); // could be non-empty-string
	} else {
		assertType('string', $x);
	}
	assertType('string', $x);

	if ((bool) array_search($x, $arr, true)) {
	} else {
		assertType('array<mixed>', $arr);
	}
	assertType('string', $x);

	if ((bool) preg_match('~.*~', $x, $matches)) {
		assertType('array{string}', $matches);
	} else {
		assertType('array{}', $matches);
	}
	// PHPantom spells PHPStan's array{}|array{string} as the one shape with an optional entry, which holds the same values.
	assertType('array{0?: string}', $matches);
}

/** @param int<-5, 5> $x */
function castString($x, string $s, bool $b) {
	if ((string) $x) {
		assertType('int<-5, 5>', $x);
	} else {
	}

	if ((string) $b) {
	} else {
	}

	if ((string) strrchr($s, 'xy')) {
		assertType('string', $s); // could be non-empty-string
	} else {
		assertType('string', $s);
	}
}

/** @param int<-5, 5> $x */
function castInt($x, string $s, bool $b) {
    if ((int) $x) {
    } else {
    }

    if ((int) $b) {
    } else {
    }

	if ((int) $s) {
		assertType('string', $s);
	} else {
		assertType('string', $s);
	}

    if ((int) strpos($s, 'xy')) {
        assertType('string', $s);
    } else {
        assertType('string', $s);
    }
}

/** @param int<-5, 5> $x */
function castFloat($x, string $s, bool $b) {
    if ((float) $x) {
        assertType('int<-5, 5>', $x);
    } else {
        assertType('int<-5, 5>', $x);
    }

    if ((float) $b) {
    } else {
    }

    if ((float) $s) {
        assertType('string', $s);
    } else {
        assertType("string", $s);
    }
}
