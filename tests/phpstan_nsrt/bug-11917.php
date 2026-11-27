<?php declare(strict_types = 1);

namespace Bug11917;

use function PHPStan\Testing\assertType;

/**
 * @return list<string>
 */
function generateList(string $name): array
{
	$a = ['a', 'b', 'c', $name];
	assertType('array{\'a\', \'b\', \'c\', string}', $a);
	$b = ['d', 'e'];
	assertType('array{\'d\', \'e\'}', $b);

	array_splice($a, 2, 0, $b);
	assertType('array{\'a\', \'b\', \'d\', \'e\', \'c\', string}', $a);

	return $a;
}

var_dump(generateList('John'));
