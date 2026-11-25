<?php

namespace Bug8559;

use function PHPStan\Testing\assertType;

class X
{
	const KEYS = ['a' => 1, 'b' => 2];

	/**
	 * @phpstan-assert key-of<self::KEYS> $key
	 * @return value-of<self::KEYS>
	 */
	public static function get(string $key): int
	{
		assert(isset(self::KEYS[$key]));
		assertType("'a'|'b'", $key);
		return self::KEYS[$key];
	}

	/**
	 * @phpstan-assert key-of<self::KEYS> $key
	 * @return value-of<self::KEYS>
	 */
	public static function get2(string $key): int
	{
		assert(in_array($key, array_keys(self::KEYS), true));
		assertType("'a'|'b'", $key);
		return self::KEYS[$key];
	}
}

$key = 'x';
$v = X::get($key);
assertType("*NEVER*", $key); // SKIP: a narrowing that rules out every value the variable holds keeps its type instead of `*NEVER*`

$key = 'a';
$v = X::get($key);
assertType("'a'", $key);
