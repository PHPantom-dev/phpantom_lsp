<?php // lint >= 8.0

namespace NativeStaticReturnType;

use function PHPStan\Testing\assertType;

class Foo
{

	public function doFoo(): static
	{
		return new static();
	}

	public function doBar(): void
	{
		assertType('static(NativeStaticReturnType\Foo)', $this->doFoo());
	}

	/**
	 * @return callable(): static
	 */
	public function doBaz(): callable
	{
		$f = function (): static {
			return new static();
		};

		assertType('static(NativeStaticReturnType\Foo)', $f());

		return $f;
	}

}

class Bar extends Foo
{

}

function (Foo $foo): void {
	assertType('NativeStaticReturnType\Foo', $foo->doFoo());

	$callable = $foo->doBaz();
	assertType('callable(): NativeStaticReturnType\Foo', $callable);
	assertType('NativeStaticReturnType\Foo', $callable());
};

function (Bar $bar): void {
	assertType('NativeStaticReturnType\Bar', $bar->doFoo());

	$callable = $bar->doBaz();
	assertType('callable(): NativeStaticReturnType\Bar', $callable);
	assertType('NativeStaticReturnType\Bar', $callable());
};
