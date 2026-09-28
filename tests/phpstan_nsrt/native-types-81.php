<?php // lint >= 8.1

namespace NativeTypes81;

use function PHPStan\Testing\assertType;
use function PHPStan\Testing\assertNativeType;


/** @return non-empty-string */
function funcWithANativeReturnType(): string
{

}

class TestFuncWithANativeReturnType
{

	public function doFoo(): void
	{
		assertType('non-empty-string', funcWithANativeReturnType());
		assertNativeType('string', funcWithANativeReturnType());

		$f = function (): string {
			return funcWithANativeReturnType();
		};

		assertNativeType('non-empty-string', $f());

			return funcWithANativeReturnType();
		})());
		assertNativeType('non-empty-string', (function (): string {
			return funcWithANativeReturnType();
		})());

		$g = fn () => funcWithANativeReturnType();

		assertType('non-empty-string', $g());
		assertNativeType('string', $g());

		assertType('non-empty-string', (fn () => funcWithANativeReturnType())());
		assertNativeType('string', (fn () => funcWithANativeReturnType())());
	}

}
