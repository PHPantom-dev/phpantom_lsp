<?php declare(strict_types = 1);

namespace Bug5333;

use function PHPStan\Testing\assertNativeType;
use function PHPStan\Testing\assertType;

class Route {}
final class FinalRoute {}

class HelloWorld
{
	/**
	 * @param Route|Route[] $foo
	 *
	 * @return Route
	 **/
	public function sayHello($foo): Route
	{
		if (\is_array($foo)) {
			$res = $foo[0];
			assertType('Bug5333\Route', $res);
			assertNativeType('mixed', $res);

			if (!$res instanceof Route) {
				throw new \Exception();
			}

			assertType('array<Bug5333\Route>', $foo);
			assertNativeType('array<mixed, mixed>', $foo);

			assertType('Bug5333\Route', $res);
			assertNativeType('Bug5333\Route', $res);

			return $res;
		}

		return $foo;
	}
}

class HelloWorld2
{
	/**
	 * @var Route|callable():Route
	 **/
	private $foo;

	/**
	 * @param Route|callable():Route $foo
	 **/
	public function setFoo($foo): void
	{
		assertType('Bug5333\Route|(callable(): Bug5333\Route)', $foo);
		assertNativeType('mixed', $foo);

		$this->foo = $foo;
	}

	public function getFoo(): Route
	{
		assertType('Bug5333\Route|(callable(): Bug5333\Route)', $this->foo);
		assertNativeType('mixed', $this->foo);

		if (\is_callable($this->foo)) {
			assertType('(Bug5333\Route&callable(): mixed)|(callable(): Bug5333\Route)', $this->foo); // SKIP: is_callable() on Foo|callable(): Foo drops the Foo&callable member for a non-final class
			assertNativeType('callable(): mixed', $this->foo);

			$res = ($this->foo)();
			assertType('mixed', $res); // SKIP: is_callable() on Foo|callable(): Foo drops the Foo&callable member for a non-final class
			assertNativeType('mixed', $res);
			if (!$res instanceof Route) {
				throw new \Exception();
			}

			return $res;
		}

		return $this->foo;
	}
}

class HelloFinalWorld
{
	/**
	 * @var FinalRoute|callable():FinalRoute
	 **/
	private $foo;

	/**
	 * @param FinalRoute|callable():FinalRoute $foo
	 **/
	public function setFoo($foo): void
	{
		assertType('Bug5333\FinalRoute|(callable(): Bug5333\FinalRoute)', $foo);
		assertNativeType('mixed', $foo);

		$this->foo = $foo;
	}

	public function getFoo(): FinalRoute
	{
		assertType('Bug5333\FinalRoute|(callable(): Bug5333\FinalRoute)', $this->foo);
		assertNativeType('mixed', $this->foo);

		if (\is_callable($this->foo)) {
			assertType('callable(): Bug5333\FinalRoute', $this->foo);
			assertNativeType('callable(): mixed', $this->foo);

			$res = ($this->foo)();
			assertType('Bug5333\FinalRoute', $res);
			assertNativeType('mixed', $res);
			if (!$res instanceof FinalRoute) {
				throw new \Exception();
			}

			return $res;
		}

		return $this->foo;
	}
}
