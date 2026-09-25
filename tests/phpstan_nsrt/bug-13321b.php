<?php // lint >= 8.2

namespace Bug13321b;

use function PHPStan\Testing\assertType;

class Foo
{
	public function __construct(
		public string $value,
		readonly public string $readonlyValue,
	)
	{
	}
}

readonly class Bar
{
	public function __construct(
		private ?Foo $foo,
	)
	{
	}

	public function bar(): void
	{
		if ($this->foo === null) {
			return;
		}
		if ($this->foo->value === '') {
			return;
		}
		if ($this->foo->readonlyValue === '') {
			return;
		}

		assertType(Foo::class, $this->foo);
		assertType('non-empty-string', $this->foo->value);
		assertType('non-empty-string', $this->foo->readonlyValue);

		$test = function () {
			assertType(Foo::class, $this->foo);
			assertType('non-empty-string', $this->foo->readonlyValue);
		};

		$test();
	}

}
