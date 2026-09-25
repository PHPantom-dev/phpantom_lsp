<?php

namespace Bug6927;

use function PHPStan\Testing\assertType;

class Foo
{
	/**
	 * @param array<non-empty-string, string> $params1
	 * @param array<non-empty-string, string> $params2
	 */
	function foo1(array $params1, array $params2): void
	{
		$params2 = array_merge($params1, $params2);

		assertType('array<non-empty-string, string>', $params2);
	}

	/**
	 * @param array<non-empty-string, string> $params1
	 * @param array<string, string> $params2
	 */
	function foo2(array $params1, array $params2): void
	{
		$params2 = array_merge($params1, $params2);

		assertType('array<string, string>', $params2);
	}

	/**
	 * @param array<string, string> $params1
	 * @param array<non-empty-string, string> $params2
	 */
	function foo3(array $params1, array $params2): void
	{
		$params2 = array_merge($params1, $params2);

		assertType('array<string, string>', $params2);
	}

	/**
	 * @param array<literal-string&non-empty-string, string> $params1
	 * @param array<non-empty-string, string> $params2
	 */
	function foo4(array $params1, array $params2): void
	{
		$params2 = array_merge($params1, $params2);

	}

	/**
	 * @param array{return: int, stdout: string, stderr: string} $params1
	 * @param array{return: int, stdout?: string, stderr?: string} $params2
	 */
	function foo5(array $params1, array $params2): void
	{
		$params3 = array_merge($params1, $params2);

		assertType('array{return: int, stdout: string, stderr: string}', $params3); // SKIP: array functions flatten the shapes they are given
	}

}
