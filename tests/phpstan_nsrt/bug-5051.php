<?php declare(strict_types = 1);

namespace Bug5051;

use function PHPStan\Testing\assertType;

class HelloWorld
{
	public function test(?object $data): void
	{
		if ($data === null) {
			$foo = 'bar';
			$update = false;
		} else {
			$foo = 'baz';
			$update = true;
		}

		assertType('object|null', $data);
		assertType("'bar'|'baz'", $foo);

		if ($update) {
			assertType('object', $data);
			assertType("'baz'", $foo);
		} else {
			assertType('null', $data);
			assertType("'bar'", $foo);
		}
		assertType('object|null', $data);
		assertType("'bar'|'baz'", $foo);
	}

	/**
	 * @param 1|2|3|10 $data
	 */
	public function testWithBooleans($data): void
	{
		$foo = 1;
		if ($data === 1 || $data === 2) {
			$update = false;
			$foo = false;
		} elseif ($data === 3) {
			$update = false;
			$foo = true;
		} else {
			$update = true;
			$foo = true;
		}

		if ($update) {
			assertType('10', $data);
			// More precise than upstream, which reports `bool`: the flags were set
			// together in each branch, so testing one recovers the other.
			assertType('true', $foo);
		} else {
			assertType('1|2|3', $data);
			assertType('bool', $foo);
		}

		if ($foo) {
			assertType('1|2|3|10', $data);
			assertType('bool', $update);
		} else {
			assertType('1|2', $data);
			// More precise than upstream, which reports `bool`: the flags were set
			// together in each branch, so testing one recovers the other.
			assertType('false', $update);
		}

		if ($data === 1) {
			// More precise than upstream, which reports `bool`: the flags were set
			// together in each branch, so testing one recovers the other.
			assertType('false', $update);
			// More precise than upstream, which reports `bool`: the flags were set
			// together in each branch, so testing one recovers the other.
			assertType('false', $foo);
		} else {
			assertType('bool', $update);
			assertType('bool', $foo);
		}

		if ($data === 2) {
			// More precise than upstream, which reports `bool`: the flags were set
			// together in each branch, so testing one recovers the other.
			assertType('false', $update);
			// More precise than upstream, which reports `bool`: the flags were set
			// together in each branch, so testing one recovers the other.
			assertType('false', $foo);
		} else {
			assertType('bool', $update);
			assertType('bool', $foo);
		}

		if ($data === 3) {
			// More precise than upstream, which reports `bool`: the flags were set
			// together in each branch, so testing one recovers the other.
			assertType('false', $update);
			assertType('true', $foo);
		} else {
			assertType('bool', $update);
			assertType('bool', $foo);
		}

		if ($data === 1 || $data === 2) {
			// More precise than upstream, which reports `bool`: the flags were set
			// together in each branch, so testing one recovers the other.
			assertType('false', $update);
			assertType('false', $foo);
		} else {
			assertType('bool', $update);
			// More precise than upstream, which reports `bool`: the flags were set
			// together in each branch, so testing one recovers the other.
			assertType('true', $foo);
		}

	}

	public function testWithDifferentVariableNames(?object $data): void
	{
		if ($data === null) {
			$update = false;
			$foo = 'bar';
		} else {
			$update = true;
			$fuu = 'baz';
		}

		if ($update) {
			assertType('object', $data);
		}
	}

	public function testWithoutExtraAssignment(?object $data): void
	{
		if ($data === null) {
			$update = false;
		} else {
			$update = true;
		}

		if ($update) {
			assertType('object', $data);
		}
	}
}
