<?php

namespace Bug778;

use PHPStan\TrinaryLogic;
use function PHPStan\Testing\assertType;
use function PHPStan\Testing\assertVariableCertainty;

class Foo
{
	/**
	 * @return resource
	 */
	public function baz()
	{
		throw new \Exception();
	}

	public function bar(): void
	{
		try {
			$handle = $this->baz();
		} finally {
			assertVariableCertainty(TrinaryLogic::createMaybe(), $handle);
		}
	}

	public function lorem(): void
	{
		try {
			$foo = foo();
		} finally {
			assertVariableCertainty(TrinaryLogic::createMaybe(), $foo);
		}

		assertVariableCertainty(TrinaryLogic::createYes(), $foo);
	}

	public function lorem2(): void
	{
		try {
			$foo = foo();
		} finally {
			$foo = 1;
		}

		assertVariableCertainty(TrinaryLogic::createYes(), $foo);
	}

	public function lorem3(): void
	{
		try {
			$foo = foo();
		} catch (\Exception $e) {
			$foo = 1;
		}

		assertVariableCertainty(TrinaryLogic::createYes(), $foo);
	}

	public function lorem4(): void
	{
		try {
			$foo = foo();
		} catch (\Exception $e) {

		}

		assertVariableCertainty(TrinaryLogic::createMaybe(), $foo);
	}

	public function ipsum(): void
	{
		try {
			doFoo();
		} catch (\InvalidArgumentException $e) {

		} catch (\RuntimeException $e) {

		}

		assertVariableCertainty(TrinaryLogic::createMaybe(), $e);
		assertType('InvalidArgumentException|RuntimeException', $e);
	}

	public function dolor(): void
	{
		try {
			doFoo();
		} catch (\InvalidArgumentException $e) {

		} catch (\RuntimeException $e) {

		} finally {

		}

		assertVariableCertainty(TrinaryLogic::createMaybe(), $e);
		assertType('InvalidArgumentException|RuntimeException', $e);
	}

	public function sit(): void
	{
		try {
			$var = 1;
		} catch (\Exception $e) {

		}

		assertVariableCertainty(TrinaryLogic::createNo(), $e);
	}
}
