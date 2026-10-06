<?php declare(strict_types = 1);

namespace Bug11687;

use function PHPStan\Testing\assertType;

class A
{
	public static function retStaticConst(): int
	{
		return 1;
	}

	/**
	 * @return static
	 */
	public static function retStatic()
	{
		return new static(); // @phpstan-ignore new.static
	}
}

class B extends A
{
	/**
	 * @return 2
	 */
	public static function retStaticConst(): int
	{
		return 2;
	}

	public function foo(): void
	{
		$clUnioned = mt_rand() === 0
			? A::class
			: X::class;

		assertType('int', A::retStaticConst());
		assertType('bool', X::retStaticConst());
		assertType('bool|int', $clUnioned::retStaticConst());

		assertType('Bug11687\A', A::retStatic());
		assertType('bool', X::retStatic());
		assertType('bool|Bug11687\A', $clUnioned::retStatic());
	}
}

class X
{
	public static function retStaticConst(): bool
	{
		return false;
	}

	public static function retStatic(): bool
	{
		return false;
	}
}
