<?php

namespace ConditionalComplexTemplates;

use function PHPStan\Testing\assertType;

/** @template T */
interface PromiseInterface
{
    /**
     * @template TFulfilled of mixed
	 * @template TRejected of mixed
     * @param (callable(T): TFulfilled)|null $onFulfilled
     * @param (callable(mixed): TRejected)|null $onRejected
     * @return PromiseInterface<(
	 *   $onFulfilled is not null
	 *     ? ($onRejected is not null ? TFulfilled|TRejected : TFulfilled)
	 *     : ($onRejected is not null ? TRejected : T)
	 * )>
     */
    public function then(callable $onFulfilled = null, callable $onRejected = null);
}

/**
 * @param PromiseInterface<true> $promise
 */
function test(PromiseInterface $promise): void
{
	$passThroughBoolFn = static fn (bool $bool): bool => $bool;

	assertType('ConditionalComplexTemplates\PromiseInterface<bool>', $promise->then($passThroughBoolFn));
	assertType('ConditionalComplexTemplates\PromiseInterface<bool>', $promise->then()->then($passThroughBoolFn));
}
