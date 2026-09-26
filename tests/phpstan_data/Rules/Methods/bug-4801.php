<?php declare(strict_types = 1);

namespace Bug4801;

use function PHPStan\Testing\assertType;

/**
 * @template T
 */
interface I {
	/**
	 * @template TResult = T
	 * @param (callable(T): TResult)|null $a
	 * @return I<TResult>
	 */
  public function work(?callable $a): I;
}

/**
 * @param I<string> $i
 */
function x(I $i) {
	assertType('Bug4801\\I<string>', $i->work(null));
	assertType('Bug4801\\I<int>', $i->work(fn(string $a) => (int) $a));
}
