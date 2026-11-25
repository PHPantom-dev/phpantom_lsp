<?php

namespace Bug10092;

use function PHPStan\Testing\assertType;

/** @template-covariant T */
interface TypeInterface {}

/** @return TypeInterface<int> */
function int() { }

/** @return TypeInterface<0> */
function zero() {  }

/** @return TypeInterface<int<1, max>> */
function positive_int() {  }

/** @return TypeInterface<numeric-string> */
function numeric_string() {  }


/**
 * @template T
 *
 * @param TypeInterface<T> $first
 * @param TypeInterface<T> $second
 * @param TypeInterface<T> ...$rest
 *
 * @return TypeInterface<T>
 */
function union(
	TypeInterface $first,
	TypeInterface $second,
	TypeInterface ...$rest
) {

}

function (): void {
	assertType('Bug10092\TypeInterface<int|numeric-string>', union(int(), numeric_string()));
	assertType('Bug10092\TypeInterface<int<0, max>>', union(positive_int(), zero()));
};
