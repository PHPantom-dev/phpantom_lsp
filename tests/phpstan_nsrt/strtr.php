<?php

namespace Strtr;

use function PHPStan\Testing\assertType;

/**
 * @param non-empty-string $nonEmptyString
 * @param non-falsy-string $nonFalseyString
 * @param mixed $mixed
 */
function doFoo(string $s, $nonEmptyString, $nonFalseyString, $mixed) {
	assertType('string', strtr($s, 'f', 'b'));
	assertType('string', strtr($s, ['f' => 'b']));
	assertType('string', strtr($s, ['f' => 'b', 'o' => 'a']));

	assertType('string', strtr($s, $s, $nonEmptyString));
	assertType('string', strtr($s, $nonEmptyString, $nonEmptyString));
	assertType('string', strtr($s, $nonFalseyString, $nonFalseyString));



	assertType('string', strtr($s, [$s => $nonEmptyString]));
	assertType('string', strtr($s, [$nonEmptyString => $nonEmptyString]));
	assertType('string', strtr($s, [$nonFalseyString => $nonFalseyString]));



	// see https://3v4l.org/jCUhW/rfc#vgit.master
	assertType('string', strtr($nonFalseyString, [$nonFalseyString => $s]));
	assertType('string', strtr($nonEmptyString, [$nonEmptyString => $s]));



	assertType('string', strtr($nonEmptyString, $mixed));
	assertType('string', strtr($nonFalseyString, $mixed));
}
