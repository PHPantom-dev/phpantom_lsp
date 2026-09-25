<?php declare(strict_types = 1);

namespace InheritDocMergingVar;

use function PHPStan\Testing\assertType;

class A {}
class B extends A {}

class One
{
	/** @var A */
	protected $property;

	public function method(): void
	{
		assertType('InheritDocMergingVar\A', $this->property);
	}
}

class Two extends One
{
	/** @var B */
	protected $property;

	public function method(): void
	{
		assertType('InheritDocMergingVar\B', $this->property);
	}
}

class Three extends Two
{
	/** Some comment */
	protected $property;

	public function method(): void
	{
		assertType('InheritDocMergingVar\B', $this->property);
	}
}

class Four extends Three
{
	protected $property;

	public function method(): void
	{
		assertType('InheritDocMergingVar\B', $this->property);
	}
}

class Five extends Four
{

	public function method(): void
	{
		assertType('InheritDocMergingVar\B', $this->property);
	}

}

class Six extends Five
{
	protected $property;

	public function method(): void
	{
		assertType('InheritDocMergingVar\B', $this->property);
	}
}

class Seven extends One
{

	/**
	 * @inheritDoc
	 * @var B
	 */
	protected $property;

}

/**
 * @template T of object
 */
class ClassWithTemplate
{

	/** @var T */
	protected $prop;

}

class ChildClassExtendingClassWithTemplate extends ClassWithTemplate
{

	protected $prop;

	public function doFoo()
	{
		assertType('object', $this->prop);
	}

}

/**
 * @extends ClassWithTemplate<\stdClass>
 */
class ChildClass2ExtendingClassWithTemplate extends ClassWithTemplate
{

	/** someComment */
	protected $prop;

	public function doFoo()
	{
		assertType('stdClass', $this->prop);
	}

}
