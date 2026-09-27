<?php

namespace Bug4500TypeInference;

use function PHPStan\Testing\assertType;

class Foo
{

	public function doFirst(): void
	{
		global $foo;
		assertType('mixed', $foo);
	}

	public function doFoo(): void
	{
		/** @var int */
		global $foo;
		assertType('int', $foo);
	}

	public function doBar(): void
	{
		/** @var int $foo */
		global $foo;
		assertType('int', $foo);
	}

	public function doBaz(): void
	{
		/** @var int */
		global $foo, $bar;
		assertType('mixed', $foo);
		assertType('mixed', $bar);
	}

	public function doLorem(): void
	{
		/** @var int $foo */
		global $foo, $bar;
		assertType('int', $foo);
		assertType('mixed', $bar);

		$baz = 'foo';

		/** @var int $baz */
		global $lorem;
		assertType('mixed', $lorem);
		assertType('\'foo\'', $baz);
	}

	public function doIpsum(): void
	{
		/**
		 * @var int $foo
		 * @var string $bar
		 */
		global $foo, $bar;

		assertType('int', $foo);
		assertType('string', $bar);
	}

	public function doDolor(): void
	{
		/** @var int $baz */
		global $lorem;

		assertType('mixed', $lorem);
		assertType('*ERROR*', $baz);
	}

	public function doSit(): void
	{
		/** @var array<int|\stdClass> $modelPropertyParameter */
		$modelPropertyParameter = doFoo();

		/** @var int $parameterIndex */
		/** @var \stdClass $modelType */
		[$parameterIndex, $modelType] = $modelPropertyParameter;

		assertType('int', $parameterIndex);
		assertType('stdClass', $modelType);
	}

	public function doAmet(array $slots): void
	{
		/** @var \stdClass[] $itemSlots */
		/** @var \stdClass[] $slots */
		$itemSlots = [];

		assertType('array<stdClass>', $itemSlots);
		assertType('array<stdClass>', $slots);
	}

	public function listDestructuring(): void
	{
		/** @var int $test */
		[[$test]] = doFoo();
		assertType('int', $test);
	}

	public function listDestructuring2(): void
	{
		/** @var int $test */
		[$test] = doFoo();
		assertType('int', $test);
	}

	public function listDestructuringForeach(): void
	{
		/** @var int $value */
		foreach (doFoo() as [[$value]]) {
			assertType('int', $value);
		}
	}

	public function listDestructuringForeach2(): void
	{
		/** @var int $value */
		foreach (doFoo() as [$value]) {
			assertType('int', $value);
		}
	}

	public function doConseteur(): void
	{
		/**
		 * @var int $foo
		 * @var string $bar
		 */
		[$foo, $bar] = doFoo();

		assertType('int', $foo);
		assertType('string', $bar);
	}

}
