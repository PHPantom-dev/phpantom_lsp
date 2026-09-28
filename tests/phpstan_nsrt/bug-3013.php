<?php declare(strict_types = 1);

namespace Bug3013;

use function PHPStan\Testing\assertType;

class HelloWorld
{
	public function test()
	{
		// Foo can be empty array, or list of ints
		$foo =  array_map('intval', $_GET['foobar']);
		assertType('array<int>', $foo);

		$bar = $this->intOrNull();
		assertType('int|null', $bar);

		if (in_array($bar, $foo, true)) {
			assertType('int', $bar);
			return;
		}
		assertType('array<int>', $foo);
		assertType('int|null', $bar);

		if (in_array($bar, $foo, true) === true) {
			assertType('int', $bar);
			return;
		}
		assertType('array<int>', $foo);
		assertType('int|null', $bar);
	}


	public function intOrNull(): ?int
	{
		return rand() === 2 ? null : rand();
	}

	/**
	 * @param array{0: 1, 1?: 2} $foo
	 */
	public function testArrayKeyExists($foo): void
	{
		assertType("array{0: 1, 1?: 2}", $foo);

		$bar = 1;
		assertType("1", $bar);

		if (array_key_exists($bar, $foo) === true) {
			assertType("array{1, 2}", $foo);
			assertType("1", $bar);
			return;
		}

		assertType("array{1}", $foo);
		assertType("1", $bar);
	}
}
