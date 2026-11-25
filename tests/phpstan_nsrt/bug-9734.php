<?php

namespace Bug9734;

use function array_is_list;
use function PHPStan\Testing\assertType;

class Foo
{

	/**
	 * @param array<mixed> $a
	 * @return void
	 */
	public function doFoo(array $a): void
	{
		if (array_is_list($a)) {
			assertType('list', $a); // SKIP: `array_is_list()` does not narrow its argument to a list
		} else {
		}
	}

	public function doFoo2(): void
	{
		$a = [];
		if (array_is_list($a)) {
			assertType('array{}', $a);
		} else {
			assertType('*NEVER*', $a); // SKIP: a branch that cannot run keeps the type instead of `*NEVER*`
		}
	}

	/**
	 * @param non-empty-array<mixed> $a
	 * @return void
	 */
	public function doFoo3(array $a): void
	{
		if (array_is_list($a)) {
		} else {
			assertType('non-empty-array<mixed>', $a);
		}
	}

	/**
	 * @param mixed $a
	 * @return void
	 */
	public function doFoo4($a): void
	{
		if (array_is_list($a)) {
			assertType('list', $a); // SKIP: `array_is_list()` does not narrow its argument to a list
		} else {
		}
	}

}

class Bar
{
	/**
	 * @param array<array-key, mixed> $value
	 * @return ($value is non-empty-list ? true : false)
	 */
	public function assertIsNonEmptyList($value): bool
	{
		return false;
	}

	/**
	 * @param array<array-key, mixed> $value
	 * @return ($value is list<string> ? true : false)
	 */
	public function assertIsStringList($value): bool
	{
		return false;
	}

	/**
	 * @param array<array-key, mixed> $value
	 * @return ($value is list{string, string} ? true : false)
	 */
	public function assertIsConstantList($value): bool
	{
		return false;
	}

	/**
	 * @param array<array-key, mixed> $value
	 * @return ($value is list{0?: string, 1?: string} ? true : false)
	 */
	public function assertIsOptionalConstantList($value): bool
	{
		return false;
	}

	/**
	 * @param array<array-key, mixed> $value
	 * @return ($value is array<string> ? true : false)
	 */
	public function assertIsStringArray($value): bool
	{
		return false;
	}

	/**
	 * @param array<mixed> $a
	 * @return void
	 */
	public function doFoo(array $a): void
	{
		if ($this->assertIsNonEmptyList($a)) {
			assertType('non-empty-list', $a);
		} else {
			assertType('array<mixed>', $a);
		}

		if ($this->assertIsStringList($a)) {
			assertType('list<string>', $a);
		} else {
		}

		if ($this->assertIsConstantList($a)) {
			assertType('array{string, string}', $a);
		} else {
			assertType('array', $a);
		}

		if ($this->assertIsOptionalConstantList($a)) {
			assertType('list{0?: string, 1?: string}', $a);
		} else {
		}

		if ($this->assertIsStringArray($a)) {
			assertType('array<string>', $a);
		} else {
		}
	}


}
