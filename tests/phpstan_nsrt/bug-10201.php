<?php // lint >= 8.1

namespace Bug10201;

use function PHPStan\Testing\assertType;

enum Hello
{
	case Hi;
}

function bla(null|string|Hello $hello): void {
	if (!in_array($hello, [Hello::Hi, null], true) && !($hello instanceof Hello)) {
		assertType('string', $hello);
	}
}

function bla2(null|string|Hello $hello): void {
	if (!in_array($hello, [Hello::Hi, null], true)) {
		assertType('string', $hello);
	}
}
