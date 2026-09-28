<?php // lint >= 8.0

namespace Bug6239;

use function PHPStan\Testing\assertNativeType;
use function PHPStan\Testing\assertType;

class UnionWithNullFails
{
	/**
	 * @param int|null|bool $value
	 */
	public function withPhpDoc(mixed $value): void
	{
		assertType('bool|int|null', $value);
		assertNativeType('mixed', $value);
	}

	public function doFoo(): void
	{
		$this->withPhpDoc(null);
	}
}
