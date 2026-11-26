<?php

namespace StringOffsets;

use function PHPStan\Testing\assertType;

/**
 * @param int<1, 3> $oneToThree
 * @param int<3, 10> $threeToTen
 * @param int<10, max> $tenOrMore
 * @param int<-10, -5> $negative
 * @param int<min, -6> $smallerMinusSix
 * @param lowercase-string $lowercase
 *
 * @return void
 */
function doFoo($oneToThree, $threeToTen, $tenOrMore, $negative, int $smallerMinusSix, int $i, string $lowercase) {
	$s = "world";
	if (rand(0, 1)) {
		$s = "hello";
	}

	assertType("''|'d'|'e'|'h'|'l'|'o'|'r'|'w'", $s[$i]);

	assertType("'h'|'w'", $s[0]);

	assertType("'e'|'l'|'o'|'r'", $s[$oneToThree]);
	// Upstream reports `*ERROR*` for an offset past the end. PHP reads `''`
	// there (with a warning), so that is the type.
	assertType("''", $s[$tenOrMore]);
	assertType("''|'d'|'l'|'o'", $s[$threeToTen]);
	// More precise than upstream, which reports `non-empty-string`: only -5
	// lands inside the string, and the offsets before it read `''`.
	assertType("''|'h'|'w'", $s[$negative]);
	assertType("''", $s[$smallerMinusSix]);

	$longString = "myF5HnJv799kWf8VRI7g97vwnABTwN9y2CzAVELCBfRqyqkdTzXg7BkGXcwuIOscAiT6tSuJGzVZOJnYXvkiKQzYBNjjkCPOzSKXR5YHRlVxV1BetqZz4XOmaH9mtacJ9azNYL6bNXezSBjX13BSZy02SK2udzQLbTPNQwlKadKaNkUxjtWegkb8QDFaXbzH1JENVSLVH0FYd6POBU82X1xu7FDDKYLzwsWJHBGVhG8iugjEGwLj22x5ViosUyKR";
	assertType("non-empty-string", $longString[$i]);

	assertType("lowercase-string&non-empty-string", $lowercase[$i]);
}

function bug12122()
{
	// see https://3v4l.org/8EMdX
	$foo = 'fo';
	// Upstream reports `*ERROR*` for an offset past either end; PHP reads `''`.
	assertType("''", $foo[2]);
	assertType("'o'", $foo[1]);
	assertType("'f'", $foo[0]);
	assertType("'o'", $foo[-1]);
	assertType("'f'", $foo[-2]);
	assertType("''", $foo[-3]);
}
