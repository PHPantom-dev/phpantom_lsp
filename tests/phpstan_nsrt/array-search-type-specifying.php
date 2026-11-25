<?php

namespace ArraySearchTypeSpecifying;

use function PHPStan\Testing\assertType;

class HelloWorld
{
	public function f($mixed, array $array, bool $b)
	{
		if (array_search("someParam", $b)) {
		} else {
			assertType('bool', $b);
		}

		if (array_search("someParam", $array)) {
		} else {
			assertType('array', $array);
		}

		if (array_search("someParam", $mixed)) {
			assertType('non-empty-array', $mixed); // SKIP: `array_search()` does not narrow its haystack to an array
		} else {
			assertType('mixed', $mixed);
		}

		if (array_search("someParam", $mixed, true)) {
			assertType('non-empty-array', $mixed); // SKIP: `array_search()` does not narrow its haystack to an array
		} else {
			assertType('mixed', $mixed);
		}

		if (array_search("someParam", $mixed) !== false) {
			assertType('non-empty-array', $mixed); // SKIP: `array_search()` does not narrow its haystack to an array
		} else {
			assertType('mixed', $mixed);
		}

		if (array_search("someParam", $mixed, true) !== false) {
			assertType('non-empty-array', $mixed); // SKIP: `array_search()` does not narrow its haystack to an array
		} else {
			assertType('mixed', $mixed);
		}
	}
}
