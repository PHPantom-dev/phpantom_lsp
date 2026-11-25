<?php

namespace ArrayFlip;

use function PHPStan\Testing\assertType;

/**
 * @param int[] $integerList
 */
function foo($integerList)
{
	$flip = array_flip($integerList);
	assertType('array<int, (int|string)>', $flip);
}

/**
 * @param mixed[] $list
 */
function foo3($list)
{
	$flip = array_flip($list);

}

/**
 * @param array<int, 1|2|3> $array
 */
function foo4($array)
{
	$flip = array_flip($array);
	assertType('array<1|2|3, int>', $flip);
}


/**
 * @param array<1|2|3, string> $array
 */
function foo5($array)
{
	$flip = array_flip($array);
	assertType('array<string, 1|2|3>', $flip);
}

/**
 * @param non-empty-array<1|2|3, 4|5|6> $array
 */
function foo6($array)
{
	$flip = array_flip($array);
}

/**
 * @param list<1|2|3> $array
 */
function foo7($array)
{
	$flip = array_flip($array);
	assertType('array<1|2|3, int<0, max>>', $flip);
}

function foo8($mixed)
{
	assertType('mixed', $mixed);
	$mixed = array_flip($mixed);
	assertType('array<int|string, int|string>', $mixed); // PHPStan says `array`; PHPantom is more precise here: the result's keys are the input's values and its values the input's keys
}

/** @param array<string, int> $array */
function foo10(array $array)
{
	if (array_key_exists('foo', $array)) {
	}

	if (array_key_exists('foo', $array) && is_int($array['foo'])) {
	}

	if (array_key_exists('foo', $array) && $array['foo'] === 17) {
	}

	if (
		array_key_exists('foo', $array) && $array['foo'] === 17
		&& array_key_exists('bar', $array) && $array['bar'] === 17
	) {
	}
}
