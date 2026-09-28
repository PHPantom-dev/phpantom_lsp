<?php

namespace Bug9662;

use function PHPStan\Testing\assertType;

/**
 * @param array<mixed> $a
 * @param array<string> $strings
 * @return void
 */
function doFoo(string $s, $a, $strings, $mixed) {
	if (in_array('foo', $a, true)) {
	} else {
	}
	assertType('array<mixed>', $a);

	if (in_array('foo', $a, false)) {
	} else {
		assertType("array<mixed>", $a);
	}
	assertType('array<mixed>', $a);

	if (in_array('foo', $a)) {
	} else {
		assertType("array<mixed>", $a);
	}
	assertType('array<mixed>', $a);

	if (in_array('0', $a)) {
	} else {
		assertType("array<mixed>", $a);
	}
	assertType('array<mixed>', $a);

	if (in_array('1', $a)) {
	} else {
		assertType("array<mixed>", $a);
	}
	assertType('array<mixed>', $a);

	if (in_array(true, $a)) {
	} else {
		assertType("array<mixed>", $a);
	}
	assertType('array<mixed>', $a);

	if (in_array(false, $a)) {
	} else {
		assertType("array<mixed>", $a);
	}
	assertType('array<mixed>', $a);

	if (in_array($s, $a, true)) {
	} else {
		assertType("array<mixed>", $a);
	}
	assertType('array<mixed>', $a);

	if (in_array($s, $a, false)) {
	} else {
		assertType("array<mixed>", $a);
	}
	assertType('array<mixed>', $a);

	if (in_array($s, $a)) {
	} else {
		assertType("array<mixed>", $a);
	}
	assertType('array<mixed>', $a);

	if (in_array($mixed, $strings, true)) {
	} else {
		assertType("array<string>", $strings);
	}
	assertType('array<string>', $strings);

	if (in_array($mixed, $strings, false)) {
		assertType('array<string>', $strings);
	} else {
		assertType("array<string>", $strings);
	}
	assertType('array<string>', $strings);

	if (in_array($mixed, $strings)) {
		assertType('array<string>', $strings);
	} else {
		assertType("array<string>", $strings);
	}
	assertType('array<string>', $strings);

	if (in_array($s, $strings, true)) {
	} else {
		assertType("array<string>", $strings);
	}
	assertType('array<string>', $strings);

	if (in_array($s, $strings, false)) {
	} else {
		assertType("array<string>", $strings);
	}
	assertType('array<string>', $strings);

	if (in_array($s, $strings)) {
	} else {
		assertType("array<string>", $strings);
	}
	assertType('array<string>', $strings);

	if (in_array($s, $strings, true) === true) {
	} else {
		assertType("array<string>", $strings);
	}
	assertType('array<string>', $strings);

	if (in_array($s, $strings, false) === true) {
	} else {
		assertType("array<string>", $strings);
	}
	assertType('array<string>', $strings);

	if (in_array($s, $strings) === true) {
	} else {
		assertType("array<string>", $strings);
	}
	assertType('array<string>', $strings);

	if (in_array($s, $strings, true) === false) {
		assertType('array<string>', $strings);
	} else {
	}
	assertType('array<string>', $strings);

	if (in_array($s, $strings, false) === false) {
		assertType('array<string>', $strings);
	} else {
	}
	assertType('array<string>', $strings);

	if (in_array($s, $strings) === false) {
		assertType('array<string>', $strings);
	} else {
	}
	assertType('array<string>', $strings);
}

/**
 * Add new delivery prices.
 *
 * @param array $price_list Prices list in multiple arrays (changed to array since 1.5.0)
 * @param bool $delete
 */
function addDeliveryPrice($price_list, $delete = false): void
{
	if (!$price_list) {
		return;
	}

	$keys = array_keys($price_list[0]);
	if (!in_array('id_shop', $keys)) {
		$keys[] = 'id_shop';
	}
	if (!in_array('id_shop_group', $keys)) {
		$keys[] = 'id_shop_group';
	}

	var_dump($keys);
}
