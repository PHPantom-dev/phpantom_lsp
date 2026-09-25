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
			assertType('bool', $foo); // SKIP: a narrowed member read or call result survives a call that can change it
		} else {
			assertType('1|2|3', $data);
			assertType('bool', $foo);
		}

		if ($foo) {
			assertType('1|2|3|10', $data);
			assertType('bool', $update);
		} else {
			assertType('1|2', $data);
			assertType('bool', $update); // SKIP: a narrowed member read or call result survives a call that can change it
		}

		if ($data === 1) {
			assertType('bool', $update); // SKIP: a narrowed member read or call result survives a call that can change it
			assertType('bool', $foo); // SKIP: a narrowed member read or call result survives a call that can change it
		} else {
			assertType('bool', $update);
			assertType('bool', $foo);
		}

		if ($data === 2) {
			assertType('bool', $update); // SKIP: a narrowed member read or call result survives a call that can change it
			assertType('bool', $foo); // SKIP: a narrowed member read or call result survives a call that can change it
		} else {
			assertType('bool', $update);
			assertType('bool', $foo);
		}

		if ($data === 3) {
			assertType('bool', $update); // SKIP: a narrowed member read or call result survives a call that can change it
			assertType('true', $foo);
		} else {
			assertType('bool', $update);
			assertType('bool', $foo);
		}

		if ($data === 1 || $data === 2) {
			assertType('bool', $update); // SKIP: a narrowed member read or call result survives a call that can change it
			assertType('false', $foo);
		} else {
			assertType('bool', $update);
			assertType('bool', $foo); // SKIP: a narrowed member read or call result survives a call that can change it
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
