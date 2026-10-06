<?php

namespace Bug4557;

use function PHPStan\Testing\assertType;

class Lorem
{
}

class Ipsum extends Lorem
{
}

interface MockObject
{
}

class Foo
{

	/**
	 * @template T
	 * @param class-string<T> $class
	 * @return T&MockObject
	 */
	public function createMock($class)
	{
	}

}

/**
 * @template T of Lorem
 */
class Bar extends Foo
{

	public function doBar(): void
	{
		$mock = $this->createMock(\stdClass::class);
		assertType('Bug4557\\MockObject&stdClass', $mock);
	}

	/** @return T */
	public function doBaz()
	{

	}

}

class Baz
{

	/**
	 * @param Bar<Lorem> $barLorem
	 * @param Bar<Ipsum> $barIpsum
	 */
	public function doFoo(Bar $barLorem, Bar $barIpsum): void
	{
		assertType('Bug4557\\Lorem', $barLorem->doBaz());
		assertType('Bug4557\\Ipsum', $barIpsum->doBaz());
	}

}
