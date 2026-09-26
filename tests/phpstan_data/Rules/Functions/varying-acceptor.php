<?php

namespace VaryingAcceptor;

use function PHPStan\Testing\assertType;

/**
 * @template T
 *
 * @param callable(callable():T):T $closure
 * @return T
 */
function bar(callable $closure) { throw new \Exception(); }

/** @param callable(callable():int):string $callable */
function testBar($callable): void {
	$a = bar($callable);
	assertType('string', $a); // can be int, but definitely not the union // SKIP: a template bound through a nested callable parameter (callable(callable(): T): T) is not inferred
}
