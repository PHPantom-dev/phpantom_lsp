<?php

namespace ValueOfGeneric;

use function PHPStan\Testing\assertType;

/**
 * @template TRow of array<string, mixed>
 */
interface Result
{
	/**
	 * @return value-of<TRow>|false
	 */
	public function getColumn();
}

/**
 * @param Result<array{k: string, j: int}> $result
 * @param Result<array{}> $emptyResult
 */
function test(
	Result $result,
	Result $emptyResult
): void {
	assertType('int|string|false', $result->getColumn());
	assertType('false', $emptyResult->getColumn());
}
