<?php declare(strict_types = 1);

namespace Bug12412;

use function PHPStan\Testing\assertType;

/**
 * @param 'A'|'B' $type
 * @return list<string>
 */
function f(string $type): array {
	$field_list = [ ];
	if ($type === 'A') {
		array_push($field_list, 'x1');
	}
	if ($type === 'B') {
		array_push($field_list, 'x2');
	}

	assertType('bool', in_array('x1', $field_list, true));

	array_push($field_list, 'x3');
	assertType('bool', in_array('x1', $field_list, true));

	return $field_list;
}
