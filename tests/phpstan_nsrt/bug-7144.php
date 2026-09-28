<?php

namespace Bug7144;

use function PHPStan\Testing\assertType;

Class Foo {
	/**
	 * @param array{foo?: array<string>, bar?: array<string>} $arr
	 */
	public function test1(array $arr): void
	{
		foreach (['foo', 'bar'] as $key) {
			\PHPStan\Testing\assertType('array{foo?: array<string>, bar?: array<string>}', $arr);
			foreach ($arr[$key] as $x) {}
		}
	}

	/**
	 * @param array{foo?: array<string>, bar?: array<string>} $arr
	 */
	public function test2(array $arr): void
	{
		foreach (['foo', 'bar', 'baz'] as $key) {
			\PHPStan\Testing\assertType('array{foo?: array<string>, bar?: array<string>}', $arr);
		}
	}

	/**
	 * @param array{foo?: array<string>, bar?: array<string>} $arr
	 */
	public function test3(array $arr): void
	{
		foreach (['foo', 'bar', 'baz'] as $key) {
			\PHPStan\Testing\assertType('array{foo?: array<string>, bar?: array<string>}', $arr);
			foreach ($arr[$key] as $x) {}
		}
	}

	/**
	 * @param 'foo'|'bar'|'baz' $key
	 * @param array{foo: array<string>, bar: array<string>} $arr
	 */
	public function test4(string $key, array $arr): void
	{
		if ($arr[$key] === []) {
			return;
		}
		\PHPStan\Testing\assertType('array{foo: array<string>, bar: array<string>}', $arr);
	}
}
