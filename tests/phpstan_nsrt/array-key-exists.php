<?php // lint >= 8.0

namespace ArrayKeyExistsExtension;

use function array_key_exists;
use function PHPStan\Testing\assertType;

class Foo
{

	/**
	 * @param array<string, string> $a
	 * @return void
	 */
	public function doFoo(array $a, string $key, int $anotherKey): void
	{
		assertType('bool', array_key_exists('foo', $a));

		$a = ['foo' => 2, 3 => 'bar'];

		if (array_key_exists($key, $a)) {
			assertType("'3'|'foo'", $key);
		}
		if (array_key_exists($anotherKey, $a)) {
			assertType('3', $anotherKey);
		}

		$empty = [];
	}

	/**
	 * @param array<int, mixed>    $a
	 * @param array<string, mixed> $b
	 * @param array<mixed>         $c
	 * @param array-key $key4
	 *
	 * @return void
	 */
	public function doBar(array $a, array $b, array $c, int $key1, string $key2, int|string $key3, $key4, mixed $key5): void
	{
		if (array_key_exists($key1, $a)) {
			assertType('int', $key1);
		}
		if (array_key_exists($key2, $a)) {
		}
		if (array_key_exists($key3, $a)) {
			assertType('int|(lowercase-string&numeric-string&uppercase-string)', $key3); // SKIP: `array_key_exists()` narrows the key by the array's declared key type, not by PHP's key casting
		}
		if (array_key_exists($key4, $a)) {
			assertType('(int|(lowercase-string&numeric-string&uppercase-string))', $key4); // SKIP: `array_key_exists()` narrows the key by the array's declared key type, not by PHP's key casting
		}
		if (array_key_exists($key5, $a)) {
			assertType('int|(lowercase-string&numeric-string&uppercase-string)', $key5); // SKIP: `array_key_exists()` narrows the key by the array's declared key type, not by PHP's key casting
		}

		if (array_key_exists($key1, $b)) {
			assertType('*NEVER*', $key1); // SKIP: a branch that cannot run keeps the type instead of `*NEVER*`
		}
		if (array_key_exists($key2, $b)) {
			assertType('string', $key2);
		}
		if (array_key_exists($key3, $b)) {
			assertType('string', $key3);
		}
		if (array_key_exists($key4, $b)) {
			assertType('string', $key4);
		}
		if (array_key_exists($key5, $b)) {
			assertType('string', $key5);
		}

		if (array_key_exists($key1, $c)) {
			assertType('int', $key1);
		}
		if (array_key_exists($key2, $c)) {
			assertType('string', $key2);
		}
		if (array_key_exists($key3, $c)) {
			assertType('(int|string)', $key3);
		}
		if (array_key_exists($key4, $c)) {
			assertType('(int|string)', $key4);
		}
		if (array_key_exists($key5, $c)) {
			assertType('(int|string)', $key5); // SKIP: `array_key_exists()` narrows the key by the array's declared key type, not by PHP's key casting
		}

		if (array_key_exists($key1, [3 => 'foo', 4 => 'bar'])) {
			assertType('3|4', $key1);
		}
		if (array_key_exists($key2, [3 => 'foo', 4 => 'bar'])) {
			assertType("'3'|'4'", $key2);
		}
		if (array_key_exists($key3, [3 => 'foo', 4 => 'bar'])) {
			assertType("3|4|'3'|'4'", $key3);
		}
		if (array_key_exists($key4, [3 => 'foo', 4 => 'bar'])) {
			assertType("(3|4|'3'|'4')", $key4);
		}
		if (array_key_exists($key5, [3 => 'foo', 4 => 'bar'])) {
			assertType("3|4|'3'|'4'", $key5);
		}
	}

}
