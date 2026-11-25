<?php

namespace GenericsDefault;

use function PHPStan\Testing\assertType;

class Foo
{

	/**
	 * @template T of int
	 * @param T $test
	 * @return T
	 */
	public function doFoo(int $test = 0): int
	{
		return $test;
	}

	public function doBar(): void
	{
		assertType('0', $this->doFoo());
		assertType('1', $this->doFoo(1));
	}

	/**
	 * @template T
	 * @param T $default
	 */
	public function doBaz($default = null): void
	{
		assertType('T (method GenericsDefault\Foo::doBaz(), argument)', $default);
	}

	/**
	 * @template T
	 * @param T|null $default
	 */
	public function doLorem($default = null): void
	{
		assertType('T (method GenericsDefault\Foo::doLorem(), argument)|null', $default);
	}

}
