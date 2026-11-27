<?php

namespace Bug11311;

use function PHPStan\Testing\assertType;
use InvalidArgumentException;

function doFoo(string $s) {
	if (1 === preg_match('/(?<major>\d+)\.(?<minor>\d+)(?:\.(?<patch>\d+))?/', $s, $matches, PREG_UNMATCHED_AS_NULL)) {

		assertType('array{0: string, major: string, 1: string, minor: string, 2: string, patch: string|null, 3: string|null}', $matches);
	}
}

function doUnmatchedAsNull(string $s): void {
	if (preg_match('/(foo)?(bar)?(baz)?/', $s, $matches, PREG_UNMATCHED_AS_NULL)) {
		assertType("array{string, 'foo'|null, 'bar'|null, 'baz'|null}", $matches);
	}
	assertType("array{}|array{string, 'foo'|null, 'bar'|null, 'baz'|null}", $matches); // SKIP: joining two branches merges shapes with different keys into one with optional keys
}

// see https://3v4l.org/VeDob
function unmatchedAsNullWithOptionalGroup(string $s): void {
	if (preg_match('/Price: (£|€)?\d+/', $s, $matches, PREG_UNMATCHED_AS_NULL)) {
		// with PREG_UNMATCHED_AS_NULL the offset 1 will always exist. It is correct that it's nullable because it's optional though
		assertType("array{string, '£'|'€'|null}", $matches);
	} else {
		assertType('array{}', $matches);
	}
	assertType("array{}|array{string, '£'|'€'|null}", $matches); // SKIP: joining two branches merges shapes with different keys into one with optional keys
}

function bug11331a(string $url):void {
	// group a is actually optional as the entire (?:...) around it is optional
	if (preg_match('{^
	(?:
		(?<a>.+)
	)?
	(?<b>.+)}mix', $url, $matches, PREG_UNMATCHED_AS_NULL)) {
		assertType('array{0: string, a: string|null, 1: string|null, b: string, 2: string}', $matches);
	}
}

function bug11331b(string $url):void {
	if (preg_match('{^
	(?:
		(?<a>.+)
	)?
	(?<b>.+)?}mix', $url, $matches, PREG_UNMATCHED_AS_NULL)) {
		assertType('array{0: string, a: string|null, 1: string|null, b: string|null, 2: string|null}', $matches);
	}
}

function bug11331c(string $url):void {
	if (preg_match('{^
	(?:
		(?:https?|git)://([^/]+)/ (?# group 1 here can be null if group 2 matches)
		|                         (?# the alternation making it so that only either should match)
		git@([^:]+):/?            (?# group 2 here can be null if group 1 matches)
	)
	([^/]+)
	/
	([^/]+?)
	(?:\.git|/)?
$}x', $url, $matches, PREG_UNMATCHED_AS_NULL)) {
		assertType('array{string, string|null, string|null, string, string}', $matches);
	}
}

class UnmatchedAsNullWithTopLevelAlternation {
	function doFoo(string $s): void {
		if (preg_match('/Price: (?:(£)|(€))\d+/', $s, $matches, PREG_UNMATCHED_AS_NULL)) {
			assertType("array{string, '£'|null, '€'|null}", $matches); // could be tagged union
		}
	}

	function doBar(string $s): void {
		if (preg_match('/Price: (?:(£)|(€))?\d+/', $s, $matches, PREG_UNMATCHED_AS_NULL)) {
			assertType("array{string, '£'|null, '€'|null}", $matches); // could be tagged union
		}
	}
}

function (string $size): void {
	if (preg_match('/ab(\d){2,4}xx([0-9])?e?/', $size, $matches, PREG_UNMATCHED_AS_NULL) !== 1) {
		throw new InvalidArgumentException(sprintf('Invalid size "%s"', $size));
	}
	assertType('array{string, string, string|null}', $matches);
};

function (string $size): void {
	if (preg_match('/a(\dAB){2}b(\d){2,4}([1-5])([1-5a-z])e?/', $size, $matches, PREG_UNMATCHED_AS_NULL) !== 1) {
		throw new InvalidArgumentException(sprintf('Invalid size "%s"', $size));
	}
	assertType('array{string, string, string, string, string}', $matches);
};

function (string $size): void {
	if (preg_match('/ab(ab(\d)){2,4}xx([0-9][a-c])?e?/', $size, $matches, PREG_UNMATCHED_AS_NULL) !== 1) {
		throw new InvalidArgumentException(sprintf('Invalid size "%s"', $size));
	}
	assertType('array{string, string, string, string|null}', $matches);
};

function (string $size): void {
	if (preg_match('/ab(\d+)e(\d?)/', $size, $matches, PREG_UNMATCHED_AS_NULL) !== 1) {
		throw new InvalidArgumentException(sprintf('Invalid size "%s"', $size));
	}
	assertType("array{string, string, string}", $matches);
};

function (string $size): void {
	if (preg_match('/ab(?P<num>\d+)e?/', $size, $matches, PREG_UNMATCHED_AS_NULL) !== 1) {
		throw new InvalidArgumentException(sprintf('Invalid size "%s"', $size));
	}
	assertType('array{0: string, num: string, 1: string}', $matches);
};

function (string $size): void {
	if (preg_match('/ab(\d\d)/', $size, $matches, PREG_UNMATCHED_AS_NULL) !== 1) {
		throw new InvalidArgumentException(sprintf('Invalid size "%s"', $size));
	}
	assertType('array{string, string}', $matches);
};

function (string $size): void {
	if (preg_match('/ab(\d+\s)e?/', $size, $matches, PREG_UNMATCHED_AS_NULL) !== 1) {
		throw new InvalidArgumentException(sprintf('Invalid size "%s"', $size));
	}
	assertType('array{string, string}', $matches);
};

function (string $size): void {
	if (preg_match('/ab(\s)e?/', $size, $matches, PREG_UNMATCHED_AS_NULL) !== 1) {
		throw new InvalidArgumentException(sprintf('Invalid size "%s"', $size));
	}
	assertType('array{string, string}', $matches);
};

function (string $size): void {
	if (preg_match('/ab(\S)e?/', $size, $matches, PREG_UNMATCHED_AS_NULL) !== 1) {
		throw new InvalidArgumentException(sprintf('Invalid size "%s"', $size));
	}
	assertType('array{string, string}', $matches);
};

function (string $size): void {
	if (preg_match('/ab(\S?)e?/', $size, $matches, PREG_UNMATCHED_AS_NULL) !== 1) {
		throw new InvalidArgumentException(sprintf('Invalid size "%s"', $size));
	}
	assertType('array{string, string}', $matches);
};

function (string $size): void {
	if (preg_match('/ab(\S)?e?/', $size, $matches, PREG_UNMATCHED_AS_NULL) !== 1) {
		throw new InvalidArgumentException(sprintf('Invalid size "%s"', $size));
	}
	assertType('array{string, string|null}', $matches);
};

function (string $size): void {
	if (preg_match('/ab(\d+\d?)e?/', $size, $matches, PREG_UNMATCHED_AS_NULL) !== 1) {
		throw new InvalidArgumentException(sprintf('Invalid size "%s"', $size));
	}
	assertType('array{string, string}', $matches);
};

function (string $s): void {
	if (preg_match('/Price: ([2-5])/i', $s, $matches, PREG_UNMATCHED_AS_NULL)) {
		assertType('array{string, string}', $matches);
	}
};

function (string $s): void {
	if (preg_match('/Price: ([2-5A-Z])/i', $s, $matches, PREG_UNMATCHED_AS_NULL)) {
		assertType('array{string, string}', $matches);
	}
};

function (string $s): void {
	if (preg_match('/^%([0-9]*\$)?[0-9]*\.?[0-9]*([sbdeEfFgGhHouxX])$/', $s, $matches, PREG_UNMATCHED_AS_NULL) === 1) {
		assertType("array{string, string|null, 'b'|'d'|'E'|'e'|'F'|'f'|'G'|'g'|'H'|'h'|'o'|'s'|'u'|'X'|'x'}", $matches);
	}
};

function (string $s): void {
	if (preg_match('/(?<whitespace>\s*)(?<value>.*)/', $s, $matches, PREG_UNMATCHED_AS_NULL) === 1) {
		assertType('array{0: string, whitespace: string, 1: string, value: string, 2: string}', $matches);
	}
};

function (string $s): void {
	preg_match('/%a(\d*)/', $s, $matches, PREG_UNMATCHED_AS_NULL);
	assertType("array{}|array{string, string}", $matches);
};

function (string $s): void {
	preg_match('/%a(\d*)?/', $s, $matches, PREG_UNMATCHED_AS_NULL);
	assertType("array{}|array{string, string|null}", $matches);
};

function (string $s): void {
	if (preg_match('~a|(\d)|(\s)~', $s, $matches, PREG_UNMATCHED_AS_NULL)) {
		assertType("array{string, string|null, string|null}", $matches);
	} else {
		assertType("array{}", $matches);
	}
	assertType("array{}|array{string, string|null, string|null}", $matches); // SKIP: joining two branches merges shapes with different keys into one with optional keys
};

function (string $s): void {
	if (preg_match('~a|(\d)|(\s)~', $s, $matches, PREG_UNMATCHED_AS_NULL|PREG_OFFSET_CAPTURE) === 1) {
		assertType("array{array{string|null, int<-1, max>}, array{string|null, int<-1, max>}, array{string|null, int<-1, max>}}", $matches);
	}
};

function (string $s): void {
	if (preg_match('~a|((u)x)|((v)y)~', $s, $matches, PREG_UNMATCHED_AS_NULL) === 1) {
		assertType("array{string, 'ux'|null, 'u'|null, 'vy'|null, 'v'|null}", $matches);
	}
};

function (string $s): void {
	preg_match('~a|(\d)|(\s)~', $s, $matches, PREG_UNMATCHED_AS_NULL);
	assertType("array{}|array{string, string|null, string|null}", $matches);
};
