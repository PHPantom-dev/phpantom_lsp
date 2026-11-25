<?php

namespace pathinfoInference;

use function PHPStan\Testing\assertType;

/**
 * @param PATHINFO_BASENAME|PATHINFO_EXTENSION $flag
 */
function doFoo(string $s, int $i, $flag) {
	assertType('array{dirname?: string, basename: string, extension?: string, filename: string}|string', pathinfo($s,  $i));
	assertType('array{dirname?: string, basename: string, extension?: string, filename: string}', pathinfo($s));

	assertType('string', pathinfo($s, PATHINFO_DIRNAME));
	assertType('string', pathinfo($s, PATHINFO_BASENAME));
	assertType('string', pathinfo($s, PATHINFO_EXTENSION));
	assertType('string', pathinfo($s, PATHINFO_FILENAME));

	assertType('string', pathinfo($s, $flag));
	if ($i === PATHINFO_ALL) {
		assertType('array{dirname?: string, basename: string, extension?: string, filename: string}', pathinfo($s, $i));
	}
	if ($i === PATHINFO_ALL || $i === PATHINFO_DIRNAME) {
		assertType('array{dirname?: string, basename: string, extension?: string, filename: string}|string', pathinfo($s, $i));
	}
}
