<?php // lint >= 8.0

declare(strict_types = 1);

namespace ArrayReverse;

use function PHPStan\Testing\assertType;

class Foo
{
	/**
	 * @param mixed[] $a
	 * @param array<string, int> $b
	 */
	public function normalArrays(array $a, array $b): void
	{
		assertType('array<mixed>', array_reverse($a));
		assertType('array<mixed>', array_reverse($a, true));

		assertType('array<string, int>', array_reverse($b));
		assertType('array<string, int>', array_reverse($b, true));
	}

	/**
	 * @param array{a: 'foo', b: 'bar', c?: 'baz'} $a
	 * @param array{17: 'foo', 19: 'bar'}|array{foo: 17, bar: 19} $b
	 * @param array{0: 'A', 1?: 'B', 2?: 'C'} $c
	 */
	public function constantArrays(array $a, array $b, array $c): void
	{
		assertType('array{}', array_reverse([]));
		assertType('array{}', array_reverse([], true));

		assertType('array{1337, null, 42}', array_reverse([42, null, 1337]));
		assertType('array{2: 1337, 1: null, 0: 42}', array_reverse([42, null, 1337], true));

		assertType('array{test3: 1337, test2: null, test1: 42}', array_reverse(['test1' => 42, 'test2' => null, 'test3' => 1337]));
		assertType('array{test3: 1337, test2: null, test1: 42}', array_reverse(['test1' => 42, 'test2' => null, 'test3' => 1337], true));

		assertType('array{test3: 1337, test2: \'test 2\', 0: 42}', array_reverse([42, 'test2' => 'test 2', 'test3' => 1337]));
		assertType('array{test3: 1337, test2: \'test 2\', 0: 42}', array_reverse([42, 'test2' => 'test 2', 'test3' => 1337], true));

		assertType('array{bar: 17, 0: 1337, foo: null, 1: 42}', array_reverse([2 => 42, 'foo' => null, 3 => 1337, 'bar' => 17]));
		assertType('array{bar: 17, 3: 1337, foo: null, 2: 42}', array_reverse([2 => 42, 'foo' => null, 3 => 1337, 'bar' => 17], true));

		assertType('array{c?: \'baz\', b: \'bar\', a: \'foo\'}', array_reverse($a));
		assertType('array{c?: \'baz\', b: \'bar\', a: \'foo\'}', array_reverse($a, true));

		assertType('array{\'bar\', \'foo\'}|array{bar: 19, foo: 17}', array_reverse($b));
		assertType('array{19: \'bar\', 17: \'foo\'}|array{bar: 19, foo: 17}', array_reverse($b, true));

		assertType("list{0: 'A'|'B'|'C', 1?: 'A'|'B', 2?: 'A'}", array_reverse($c));
		assertType("array{2?: 'C', 1?: 'B', 0: 'A'}", array_reverse($c, true));
	}

	/**
	 * @param list<string> $a
	 * @param non-empty-list<string> $b
	 */
	public function list(array $a, array $b): void
	{
		assertType('list<string>', array_reverse($a));
		assertType('array<int<0, max>, string>', array_reverse($a, true));

		assertType('non-empty-list<string>', array_reverse($b));
		assertType('non-empty-array<int<0, max>, string>', array_reverse($b, true));
	}

	public function mixed(mixed $mixed): void
	{
		assertType('array<mixed, mixed>', array_reverse($mixed));
		assertType('array<mixed, mixed>', array_reverse($mixed, true));

		if (array_key_exists('foo', $mixed)) {
			assertType('non-empty-array', array_reverse($mixed)); // SKIP: `array_key_exists()` does not narrow `mixed` to an array
			assertType("non-empty-array&hasOffset('foo')", array_reverse($mixed, true)); // SKIP: `array_key_exists()` does not narrow `mixed` to an array
		}
	}
}
