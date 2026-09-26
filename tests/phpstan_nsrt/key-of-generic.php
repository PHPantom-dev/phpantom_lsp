<?php

namespace KeyOfGeneric;

use function PHPStan\Testing\assertType;

/**
 * @template TRow of array<mixed>
 */
interface Result
{
	/**
	 * @return key-of<TRow>|null
	 */
	public function getKey();
}

/**
 * @param Result<array{k: string, j: int}> $result
 * @param Result<array{int, string}> $listResult
 * @param Result<array<mixed>> $mixedResult
 * @param Result<array<string, mixed>> $stringKeyResult
 * @param Result<array<int, mixed>> $intKeyResult
 * @param Result<array{}> $emptyResult
 */
function test(
	Result $result,
	Result $listResult,
	Result $mixedResult,
	Result $stringKeyResult,
	Result $intKeyResult,
	Result $emptyResult,
): void {
	assertType("'j'|'k'|null", $result->getKey());
	assertType('0|1|null', $listResult->getKey());
	assertType('int|string|null', $mixedResult->getKey());
	assertType('string|null', $stringKeyResult->getKey());
	assertType('int|null', $intKeyResult->getKey());
	assertType('null', $emptyResult->getKey());
}
