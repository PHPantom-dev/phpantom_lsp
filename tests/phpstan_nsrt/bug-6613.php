<?php

namespace Bug6613;

use function PHPStan\Testing\assertType;

function (\DateTime $dt) {
	assertType("'000000'", date('u'));
	assertType('non-falsy-string&numeric-string', date_format($dt, 'u'));
	assertType('non-falsy-string&numeric-string', $dt->format('u'));

	assertType("'000'", date('v'));
	assertType('non-falsy-string&numeric-string', date_format($dt, 'v'));
	assertType('non-falsy-string&numeric-string', $dt->format('v'));
};
