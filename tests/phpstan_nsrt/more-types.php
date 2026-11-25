<?php

namespace MoreTypes;

use function PHPStan\Testing\assertType;

class Foo
{

	/**
	 * @param pure-callable $pureCallable
	 * @param callable-array $callableArray
	 * @param closed-resource $closedResource
	 * @param open-resource $openResource
	 * @param enum-string $enumString
	 * @param non-empty-literal-string $nonEmptyLiteralString
	 * @param non-empty-scalar $nonEmptyScalar
	 * @param empty-scalar $emptyScalar
	 * @param non-empty-mixed $nonEmptyMixed
	 * @param lowercase-string $lowercaseString
	 * @param non-empty-lowercase-string $nonEmptyLowercaseString
	 * @param uppercase-string $uppercaseString
	 * @param non-empty-uppercase-string $nonEmptyUppercaseString
	 */
	public function doFoo(
		$pureCallable,
		$callableArray,
		$closedResource,
		$openResource,
		$enumString,
		$nonEmptyLiteralString,
		$nonEmptyScalar,
		$emptyScalar,
		$nonEmptyMixed,
		$lowercaseString,
		$nonEmptyLowercaseString,
		$uppercaseString,
		$nonEmptyUppercaseString,
	): void
	{
		assertType('pure-callable(): mixed', $pureCallable); // SKIP
		assertType('array&callable(): mixed', $callableArray); // SKIP
		assertType('resource', $closedResource); // SKIP
		assertType('resource', $openResource); // SKIP
		assertType('class-string<UnitEnum>', $enumString); // SKIP
		assertType('literal-string&non-empty-string', $nonEmptyLiteralString); // SKIP
		assertType('float|int<min, -1>|int<1, max>|non-falsy-string|true', $nonEmptyScalar); // SKIP
		assertType("0|0.0|''|'0'|false", $emptyScalar); // SKIP
		assertType("mixed~(0|0.0|''|'0'|array{}|false|null)", $nonEmptyMixed); // SKIP
		assertType('lowercase-string', $lowercaseString);
		assertType('lowercase-string&non-empty-string', $nonEmptyLowercaseString); // SKIP
		assertType('uppercase-string', $uppercaseString);
		assertType('non-empty-string&uppercase-string', $nonEmptyUppercaseString); // SKIP
	}

}
