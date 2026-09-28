<?php

namespace NativeExpressions;

use function PHPStan\Testing\assertType;
use function PHPStan\Testing\assertNativeType;

function doFoo(): string
{
}

function (): void {
	/** @var non-empty-string $a */
	$a = doFoo();
	assertType('non-empty-string', $a);
	assertNativeType('mixed', $a); // could be fixed
};

/**
 * @param positive-int|non-empty-string $a
 */
function foo(int|string $a): void
{
	assertNativeType('int|string', $a);
	if (is_string($a)) {
		assertType('non-empty-string', $a);
		assertNativeType('string', $a);
	}
}

class Foo{
	public function __construct(
		/** @var non-empty-array<mixed> */
		private array $array
	){
		assertType('non-empty-array<mixed>', $this->array);
		assertNativeType('array', $this->array);
		if(count($array) === 0){
			throw new \InvalidArgumentException();
		}
	}

	/**
	 * @param array{a: 'b'} $a
	 * @return void
	 */
	public function doUnset(array $a){
		assertType("array{a: 'b'}", $a);
		assertNativeType('array', $a);
		unset($a['a']);
		assertType("array{}", $a);
		assertNativeType("array<mixed~'a', mixed>", $a);
	}
}

