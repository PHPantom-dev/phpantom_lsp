<?php declare(strict_types = 1);

namespace Bug6623;

use function PHPStan\Testing\assertType;

class World {
	protected function bar(): void {}
}

class HelloWorld extends World
{
	private ?int $foo = null;

	public function useFoo(): void
	{
		if (is_null($this->foo)) {
			throw new \LogicException();
		}
		assertType('int', $this->foo);
		$this->bar();
		assertType('int', $this->foo);
	}
}
