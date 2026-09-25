<?php

declare(strict_types=1);

namespace Ctype;

use function ctype_alnum;
use function ctype_alpha;
use function ctype_cntrl;
use function ctype_graph;
use function ctype_lower;
use function ctype_print;
use function ctype_punct;
use function ctype_space;
use function ctype_upper;
use function ctype_xdigit;
use function PHPStan\Testing\assertType;

function test(string $str): void
{
	// functions asserting non-falsy-string

	if (ctype_alpha($str)) {
	} else {
		assertType('string', $str);
	}

	if (ctype_cntrl($str)) {
	} else {
		assertType('string', $str);
	}

	if (ctype_lower($str)) {
	} else {
		assertType('string', $str);
	}

	if (ctype_upper($str)) {
	} else {
		assertType('string', $str);
	}

	if (ctype_punct($str)) {
	} else {
		assertType('string', $str);
	}

	if (ctype_space($str)) {
	} else {
		assertType('string', $str);
	}

	// functions asserting non-empty-string
	// ctype_digit is tested in another file

	if (ctype_alnum($str)) {
	} else {
		assertType('string', $str);
	}

	if (ctype_graph($str)) {
	} else {
		assertType('string', $str);
	}

	if (ctype_print($str)) {
	} else {
		assertType('string', $str);
	}

	if (ctype_xdigit($str)) {
	} else {
		assertType('string', $str);
	}
}

function testInt(int $int): void
{
	if (ctype_alpha($int)) {
		assertType('int', $int);
	} else {
		assertType('int', $int);
	}

	if (ctype_cntrl($int)) {
		assertType('int', $int);
	} else {
		assertType('int', $int);
	}

	if (ctype_lower($int)) {
		assertType('int', $int);
	} else {
		assertType('int', $int);
	}

	if (ctype_upper($int)) {
		assertType('int', $int);
	} else {
		assertType('int', $int);
	}

	if (ctype_punct($int)) {
		assertType('int', $int);
	} else {
		assertType('int', $int);
	}

	if (ctype_space($int)) {
		assertType('int', $int);
	} else {
		assertType('int', $int);
	}

	// functions asserting non-empty-string
	// ctype_digit is tested in another file

	if (ctype_alnum($int)) {
		assertType('int', $int);
	} else {
		assertType('int', $int);
	}

	if (ctype_graph($int)) {
		assertType('int', $int);
	} else {
		assertType('int', $int);
	}

	if (ctype_print($int)) {
		assertType('int', $int);
	} else {
		assertType('int', $int);
	}

	if (ctype_xdigit($int)) {
		assertType('int', $int);
	} else {
		assertType('int', $int);
	}
}
