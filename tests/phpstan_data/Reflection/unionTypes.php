<?php // lint >= 8.0

namespace NativeUnionTypes;

use function PHPStan\Testing\assertNativeType;
use function PHPStan\Testing\assertType;

class Foo
{

	public int|bool $fooProp;

	public function doFoo(int|bool $foo): self|Bar
	{
		assertType('bool|int', $foo);
		assertType('bool|int', $this->fooProp);
		assertNativeType('bool|int', $foo);
	}

}

class Bar
{

}

function doFoo(int|bool $foo): Foo|Bar
{
	assertType('bool|int', $foo);
	assertNativeType('bool|int', $foo);
}

function (Foo $foo): void {
	assertType('bool|int', $foo->fooProp);
	assertType('NativeUnionTypes\\Bar|NativeUnionTypes\\Foo', $foo->doFoo(1));
	assertType('NativeUnionTypes\\Bar|NativeUnionTypes\\Foo', doFoo(1));
};

function (): void {
	$f = function (int|bool $foo): Foo|Bar {
		assertType('bool|int', $foo);
	};

	assertType('NativeUnionTypes\\Bar|NativeUnionTypes\\Foo', $f(1));
};

class Baz
{

	public function doFoo(array|false $foo): void
	{
		assertType('array|false', $foo);
		assertNativeType('array|false', $foo);
		assertType('array|false', $this->doBar());
	}

	public function doBar(): array|false
	{

	}

	/**
	 * @param array<int, string> $foo
	 */
	public function doBaz(array|false $foo): void
	{
		assertType('array<int, string>|false', $foo);
		assertNativeType('array|false', $foo);

		assertType('array<int, string>|false', $this->doLorem());
	}

	/**
	 * @return array<int, string>
	 */
	public function doLorem(): array|false
	{

	}

	public function doIpsum(int|string|null $nullable): void
	{
		assertType('int|string|null', $nullable);
		assertNativeType('int|string|null', $nullable);
		assertType('int|string|null', $this->doDolor());
	}

	public function doDolor(): int|string|null
	{

	}

}
