<?php // lint >= 8.0

namespace Bug2600Php8;

use function PHPStan\Testing\assertType;

class Foo
{
	/**
	 * @param mixed ...$x
	 */
	public function doFoo($x = null) {
		$args = func_get_args();
		assertType('mixed', $x);
		assertType('list<mixed>', $args);
	}

	/**
	 * @param mixed ...$x
	 */
	public function doBar($x = null) {
		assertType('mixed', $x);
	}

	/**
	 * @param mixed $x
	 */
	public function doBaz(...$x) {
		assertType('array<int|string, mixed>', $x);
	}

	/**
	 * @param mixed ...$x
	 */
	public function doLorem(...$x) {
		assertType('array<int|string, mixed>', $x);
	}

	public function doIpsum($x = null) {
		$args = func_get_args();
		assertType('mixed', $x);
		assertType('list<mixed>', $args);
	}
}

class Bar
{
	/**
	 * @param string ...$x
	 */
	public function doFoo($x = null) {
		$args = func_get_args();
		assertType('string|null', $x);
		assertType('list<mixed>', $args);
	}

	/**
	 * @param string ...$x
	 */
	public function doBar($x = null) {
		assertType('string|null', $x);
	}

	/**
	 * @param string $x
	 */
	public function doBaz(...$x) {
		assertType('array<int|string, string>', $x);
	}

	/**
	 * @param string ...$x
	 */
	public function doLorem(...$x) {
		assertType('array<int|string, string>', $x);
	}
}

function foo($x, string ...$y): void
{
	assertType('mixed', $x);
	assertType('array<int|string, string>', $y);
}

function ($x, string ...$y): void {
	assertType('mixed', $x);
	assertType('array<int|string, string>', $y);
};
