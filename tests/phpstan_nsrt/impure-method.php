<?php

namespace ImpureMethod;

use stdClass;
use function PHPStan\Testing\assertType;

/**
 * @method $this phpDocReturnThis()
 */
class Foo
{

	/** @var int */
	private $fooProp;

	public function voidMethod(): void
	{
		$this->fooProp = rand(0, 1);
	}

	/**
	 * @return $this
	 */
	public function returnsThis($arg)
	{
		$this->fooProp = rand(0, 1);
	}

	/**
	 * @return $this
	 * @phpstan-impure
	 */
	public function returnsThisImpure($arg)
	{
		$this->fooProp = rand(0, 1);
	}

	public function ordinaryMethod(): int
	{
		return 1;
	}

	/**
	 * @phpstan-impure
	 * @return int
	 */
	public function impureMethod(): int
	{
		$this->fooProp = rand(0, 1);

		return $this->fooProp;
	}

	/**
	 * @impure
	 * @return int
	 */
	public function impureMethod2(): int
	{
		$this->fooProp = rand(0, 1);

		return $this->fooProp;
	}

	public function doFoo(): void
	{
		$this->fooProp = 1;
		assertType('1', $this->fooProp);

		$this->voidMethod();
		assertType('int', $this->fooProp);
	}

	public function doFluent(): void
	{
		$this->fooProp = 1;
		assertType('1', $this->fooProp);

		$this->returnsThis(new stdClass());
		assertType('int', $this->fooProp);
	}

	public function doFluent2(): void
	{
		$this->fooProp = 1;
		assertType('1', $this->fooProp);

		$this->phpDocReturnThis();
		assertType('int', $this->fooProp);
	}

	public function doBar(): void
	{
		$this->fooProp = 1;
		assertType('1', $this->fooProp);

		$this->ordinaryMethod();
		assertType('1', $this->fooProp);
	}

	public function doBaz(): void
	{
		$this->fooProp = 1;
		assertType('1', $this->fooProp);

		$this->impureMethod();
		assertType('int', $this->fooProp);
	}

	public function doLorem(): void
	{
		$this->fooProp = 1;
		assertType('1', $this->fooProp);

		$this->impureMethod2();
		assertType('int', $this->fooProp);
	}

}

class Person
{

	public function getName(): ?string
	{
	}

}

class Bar
{

	public function doFoo(): void
	{
		$f = new Foo();

		$p = new Person();
		assert($p->getName() !== null);
		assertType('string', $p->getName());
		$f->returnsThis($p);
		assertType('string', $p->getName());
	}

	public function doFoo2(): void
	{
		$f = new Foo();

		$p = new Person();
		assert($p->getName() !== null);
		assertType('string', $p->getName());
		$f->returnsThisImpure($p);
		assertType('string|null', $p->getName());
	}

}

class ToBeExtended
{

	/** @phpstan-pure */
	public function pure(): int
	{

	}

	/** @phpstan-impure */
	public function impure(): int
	{
		echo 'test';
		return 1;
	}

}

class ExtendingClass extends ToBeExtended
{

	/**
	 * @return int
	 */
	public function pure(): int
	{
		echo 'test';
		return 1;
	}

	/**
	 * @return int
	 */
	public function impure(): int
	{
		return 1;
	}

}

function (ExtendingClass $e): void {
	assert($e->pure() === 1);
	assertType('1', $e->pure());
	$e->impure();
	assertType('int', $e->pure());
};
