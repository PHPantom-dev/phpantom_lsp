<?php

namespace Bug3789;

use function PHPStan\Testing\assertType;

/**
 * @param array{string, string, string} $haystack
 */
function doFoo(string $needle, array $haystack): void {
	assertType('0|1|2|false', array_search($needle, $haystack, true));
	assertType('0|1|2|false', array_search('foo', $haystack, true));

	assertType('0|1|2|false', array_search($needle, $haystack));
	assertType('0|1|2|false', array_search('foo', $haystack));

	$haystack[1] = 'foo';
	assertType('0|1|2|false', array_search($needle, $haystack, true));
	assertType('0|1', array_search('foo', $haystack, true)); // PHPStan says `0|1|2`, but the search stops at index 1, which is certain to match

	assertType('0|1|2|false', array_search($needle, $haystack));
	assertType('0|1', array_search('foo', $haystack)); // PHPStan says `0|1|2`, but the search stops at index 1, which is certain to match
}
