<?php

namespace ImpureConstuctor;

use function PHPStan\Testing\assertType;

class Foo
{

	/** @var bool */
	private $active;

	public function __construct()
	{
		$this->active = false;
	}

	public function getActive(): bool
	{
		return $this->active;
	}

	public function activate(): void
	{
		$this->active = true;
	}

}


class ClassWithImpureConstructorNotMarked
{

	public function __construct(Foo $foo)
	{
		$foo->activate();
	}

}

class ClassWithImpureConstructorMarked
{

	/**
	 * @phpstan-impure
	 */
	public function __construct(Foo $foo)
	{
		$foo->activate();
	}

}

class ClassWithPureConstructorMarked
{

	/**
	 * @phpstan-pure
	 */
	public function __construct(Foo $foo)
	{
	}

}

class ClassWithImpureConstructorNotMarkedWithoutParameters
{

	/** @var string */
	private $lorem;

	public function __construct()
	{
		$this->lorem = 'lorem';
	}

}

class Test
{

	public function testClassWithImpureConstructorNotMarked()
	{
		$foo = new Foo();
		assertType('bool', $foo->getActive());

		assert(!$foo->getActive());
		assertType('false', $foo->getActive());

		new ClassWithImpureConstructorNotMarked($foo);
		assertType('false', $foo->getActive());
	}

	public function testClassWithImpureConstructorMarked()
	{
		$foo = new Foo();
		assertType('bool', $foo->getActive());

		assert(!$foo->getActive());
		assertType('false', $foo->getActive());

		new ClassWithImpureConstructorMarked($foo);
		assertType('bool', $foo->getActive());
	}

	public function testClassWithPureConstructorMarked()
	{
		$foo = new Foo();
		assertType('bool', $foo->getActive());

		assert(!$foo->getActive());
		assertType('false', $foo->getActive());

		new ClassWithPureConstructorMarked($foo);
		assertType('false', $foo->getActive());
	}

	public function testClassWithImpureConstructorNotMarkedWithoutParameters()
	{
		$foo = new Foo();
		assertType('bool', $foo->getActive());

		assert(!$foo->getActive());
		assertType('false', $foo->getActive());

		new ClassWithImpureConstructorNotMarkedWithoutParameters();
		assertType('false', $foo->getActive());
	}

}
