<?php

namespace Bug9828;

use function PHPStan\Testing\assertType;

/**
 * @template T of array{value: int, ...}
 * @param array<int, T> $array
 * @return array<int, T>
 */
function filter(array $array, int $min_value): array
{
	foreach ($array as $key => $row) {
		if ($row['value'] < $min_value) unset($array[$key]);
	}
	return array_values($array);
}

/**
 * @template T of array{value: int, ...}
 * @param array<int, T> $array
 * @return list<T>
 */
function filter2(array $array, int $min_value): array
{
	$result = [];
	foreach ($array as $row) {
		if ($row['value'] >= $min_value) $result[] = $row;
	}
	return $result;
}

/**
 * @param array<int, array{value: int, name: string}> $data
 */
function test(array $data): void
{
	assertType('array<int, array{value: int, name: string}>', filter($data, 5));
	assertType('list<array{value: int, name: string}>', filter2($data, 5));
}
