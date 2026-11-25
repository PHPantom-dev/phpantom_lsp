<?php // lint >= 8.1

namespace Bug9662Enums;

use function PHPStan\Testing\assertType;

enum Suit
{
	case Hearts;
	case Diamonds;
	case Clubs;
	case Spades;
}

/**
 * @param array<Suit> $suite
 */
function doEnum(array $suite, array $arr) {
	if (in_array('NotAnEnumCase', $suite) === false) {
		assertType('array<Bug9662Enums\Suit>', $suite);
	} else {
	}
	assertType('array<Bug9662Enums\Suit>', $suite);

	if (in_array(Suit::Hearts, $suite) === false) {
	} else {
	}
	assertType('array<Bug9662Enums\Suit>', $suite);


	if (in_array(Suit::Hearts, $arr) === false) {
	} else {
	}
	assertType('array', $arr);


	if (in_array('NotAnEnumCase', $arr) === false) {
		assertType('array', $arr);
	} else {
	}
	assertType('array', $arr);
}

enum StringBackedSuit: string
{
	case Hearts = 'H';
	case Diamonds = 'D';
	case Clubs = 'C';
	case Spades = 'S';
}

/**
 * @param array<StringBackedSuit> $suite
 */
function doBackedEnum(array $suite, array $arr, string $s, int $i, $mixed) {
	if (in_array('NotAnEnumCase', $suite) === false) {
		assertType('array<Bug9662Enums\StringBackedSuit>', $suite);
	} else {
	}
	assertType('array<Bug9662Enums\StringBackedSuit>', $suite);

	if (in_array(StringBackedSuit::Hearts, $suite) === false) {
	} else {
	}
	assertType('array<Bug9662Enums\StringBackedSuit>', $suite);


	if (in_array($s, $suite) === false) {
		assertType('array<Bug9662Enums\StringBackedSuit>', $suite);
	} else {
	}
	assertType('array<Bug9662Enums\StringBackedSuit>', $suite);

	if (in_array($i, $suite) === false) {
		assertType('array<Bug9662Enums\StringBackedSuit>', $suite);
	} else {
	}
	assertType('array<Bug9662Enums\StringBackedSuit>', $suite);

	if (in_array($mixed, $suite) === false) {
		assertType('array<Bug9662Enums\StringBackedSuit>', $suite);
	} else {
	}
	assertType('array<Bug9662Enums\StringBackedSuit>', $suite);


	if (in_array(StringBackedSuit::Hearts, $arr) === false) {
	} else {
	}
	assertType('array', $arr);
}
