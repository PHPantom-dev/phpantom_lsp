<?php declare(strict_types = 1);

namespace Bug9634;

use function PHPStan\Testing\assertType;

/** @template T */
interface Option {
	/** @return self<never> */
	static function none(): self;

	/** @return T */
	function unwrap(): mixed;

	/**
	 * @return (T is never ? false : bool)
	 */
	function isSome(): bool;
}

/** @param Option<never> $o */
function f(Option $o): void {
	assertType('false', $o->isSome());
}

/** @param Option<int> $o */
function g(Option $o): void {
	assertType('bool', $o->isSome());
}
