<?php

namespace DoNotRememberImpureFunctions;

use function PHPStan\Testing\assertType;

class Foo
{

	public function doFoo()
	{
		function (): void {
			if (rand(0, 1)) {
			}
		};

		function (): void {
			if (rand(0, 1) === 0) {
			}
		};
		function (): void {
		};
	}

	public function doBar(): bool
	{

	}

	/** @phpstan-pure */
	public function doBaz(): bool
	{

	}

	/** @phpstan-impure */
	public function doLorem(): bool
	{

	}

	public function doIpsum()
	{
		if ($this->doBar() === true) {
			assertType('true', $this->doBar());
		}

		if ($this->doBaz() === true) {
			assertType('true', $this->doBaz());
		}

		if ($this->doLorem() === true) {
			assertType('bool', $this->doLorem());
		}
	}

	public function doDolor()
	{
		if ($this->doBar()) {
			assertType('true', $this->doBar());
		}

		if ($this->doBaz()) {
			assertType('true', $this->doBaz());
		}

		if ($this->doLorem()) {
			assertType('bool', $this->doLorem());
		}
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
