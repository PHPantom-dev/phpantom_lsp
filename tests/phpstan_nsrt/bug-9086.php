<?php // lint >= 8.0

namespace Bug9086;

use ArrayObject;
use function PHPStan\Testing\assertType;

/**
 * @template A
 * @template B
 *
 * @param A $items
 * @param callable(A): B $ab
 * @return B
 */
function pipe(mixed $items, callable $ab): mixed
{
	return $ab($items);
}

/**
 * @return ArrayObject<string, bool>
 */
function getObject(): ArrayObject
{
	return new ArrayObject;
}

function (): void {
	$result = pipe(getObject(), function(ArrayObject $i) {
		assertType('ArrayObject<string, bool>', $i);
		return $i;
	});

	assertType('ArrayObject<string, bool>', $result);
};
