<?php // lint >= 8.1

declare(strict_types = 0);

namespace Bug12902NonStrict;

use function PHPStan\Testing\assertNativeType;
use function PHPStan\Testing\assertType;

class NarrowsNativeConstantValue
{
	private readonly int|float $i;

	public function __construct()
	{
		$this->i = 1;
	}

	public function doFoo(): void
	{
		assertType('1', $this->i);
		assertNativeType('1', $this->i);
	}
}

class NarrowsNativeReadonlyUnion {
	private readonly int|float $i;

	public function __construct()
	{
		$this->i = getInt();
		assertType('int', $this->i);
		assertNativeType('int', $this->i);
	}

	public function doFoo(): void {
		assertType('int', $this->i);
		assertNativeType('int', $this->i);
	}
}

class NarrowsNativeUnion {
	private int|float $i;

	public function __construct()
	{
		$this->i = getInt();
		assertType('int', $this->i);
		assertNativeType('int', $this->i);

		$this->impureCall();
		assertType('float|int', $this->i);
		assertNativeType('float|int', $this->i);
	}

	public function doFoo(): void {
		assertType('float|int', $this->i);
		assertNativeType('float|int', $this->i);
	}

	/** @phpstan-impure  */
	public function impureCall(): void {}
}

class NarrowsStaticNativeUnion {
	private static int|float $i;

	public function __construct()
	{
		self::$i = getInt();
		assertType('int', self::$i);
		assertNativeType('int', self::$i);

		$this->impureCall();
		assertType('float|int', self::$i);
		assertNativeType('float|int', self::$i);
	}

	public function doFoo(): void {
		assertType('float|int', self::$i);
		assertNativeType('float|int', self::$i);
	}

	/** @phpstan-impure  */
	public function impureCall(): void {}
}

function getInt(): int {
	return 1;
}
