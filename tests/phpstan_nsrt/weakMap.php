<?php // lint >= 8.0

declare(strict_types = 1);

namespace weakMap;

use WeakMap;
use function PHPStan\Testing\assertType;

interface Foo {}
interface Bar {}

/**
 * @param WeakMap<Foo, Bar> $weakMap
 */
function weakMapOffsetGetNotNullable(WeakMap $weakMap, Foo $foo): void
{
	$bar = $weakMap[$foo];

	assertType(Bar::class, $bar);
}


/**
 * @param WeakMap<Foo, Bar|null> $weakMap
 */
function weakMapOffsetGetNullable(WeakMap $weakMap, Foo $foo): void
{
	$bar = $weakMap[$foo];

	assertType( 'weakMap\\Bar|null', $bar);
}
