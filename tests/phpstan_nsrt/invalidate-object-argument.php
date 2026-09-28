<?php

namespace InvalidateObjectArgument;

use function PHPStan\Testing\assertType;

class Foo
{

	public function getName(): string
	{

	}

}

class Bar
{

	/**
	 * @param object $foo
	 */
	public function doFoo($foo)
	{
		assert($foo instanceof Foo);
		assertType(Foo::class, $foo);
		assertType('string', $foo->getName());
		assert($foo->getName() === 'foo');
		assertType('\'foo\'', $foo->getName());

		$this->doBar($foo);
		assertType('\'foo\'', $foo->getName());
		assertType(Foo::class, $foo);

		$this->doBaz($foo);
		assertType('\'foo\'', $foo->getName());
		assertType(Foo::class, $foo);

		assert($foo->getName() === 'foo');
		assertType('\'foo\'', $foo->getName());

		$this->doLorem($foo);
		assertType('string', $foo->getName());
		assertType(Foo::class, $foo);

		assert($foo->getName() === 'foo');
		assertType('\'foo\'', $foo->getName());

		$this->doIpsum($foo);
		assertType('string', $foo->getName());
		assertType(Foo::class, $foo);
	}

	/**
	 * @phpstan-pure
	 */
	public function doBar($arg)
	{

	}

	public function doBaz($arg)
	{

	}

	public function doLorem($arg): void
	{

	}

	/** @phpstan-impure */
	public function doIpsum($arg)
	{

	}

}
