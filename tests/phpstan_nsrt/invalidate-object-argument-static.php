<?php

namespace InvalidateObjectArgumentStatic;

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

		self::doBar($foo);
		assertType('\'foo\'', $foo->getName());
		assertType(Foo::class, $foo);

		self::doBaz($foo);
		assertType('\'foo\'', $foo->getName());
		assertType(Foo::class, $foo);

		assert($foo->getName() === 'foo');
		assertType('\'foo\'', $foo->getName());

		self::doLorem($foo);
		assertType('string', $foo->getName());
		assertType(Foo::class, $foo);

		assert($foo->getName() === 'foo');
		assertType('\'foo\'', $foo->getName());

		self::doIpsum($foo);
		assertType('string', $foo->getName());
		assertType(Foo::class, $foo);
	}

	/**
	 * @phpstan-pure
	 */
	public static function doBar($arg)
	{

	}

	public static function doBaz($arg)
	{

	}

	public static function doLorem($arg): void
	{

	}

	/** @phpstan-impure */
	public static function doIpsum($arg)
	{

	}

}
