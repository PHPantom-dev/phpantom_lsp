<?php

namespace Bug6576;

use function PHPStan\Testing\assertType;

/**
 * @param array<int|string, mixed> $arr
 */
function alreadyWorks(array $arr): void {
	foreach ($arr as $key => $value) {
		assertType('int|string', $key);
	}
}

/**
 * @template ArrType of array<int|string, mixed>
 *
 * @param ArrType $arr
 */
function shouldWork(array $arr): void {
	foreach ($arr as $key => $value) {
		assertType('int|string', $key);
	}
}
