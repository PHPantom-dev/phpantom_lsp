<?php

namespace InvalidateObjectArgumentStaticFunction;

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

		doBar($foo);
		assertType('\'foo\'', $foo->getName());
		assertType(Foo::class, $foo);

		doBaz($foo);
		assertType('\'foo\'', $foo->getName());
		assertType(Foo::class, $foo);

		assert($foo->getName() === 'foo');
		assertType('\'foo\'', $foo->getName());

		doLorem($foo);
		assertType('string', $foo->getName());
		assertType(Foo::class, $foo);

		assert($foo->getName() === 'foo');
		assertType('\'foo\'', $foo->getName());

		doIpsum($foo);
		assertType('string', $foo->getName());
		assertType(Foo::class, $foo);
	}

}

/**
 * @phpstan-pure
 */
function doBar($arg)
{

}

function doBaz($arg)
{

}

function doLorem($arg): void
{

}

/** @phpstan-impure */
function doIpsum($arg)
{

}
