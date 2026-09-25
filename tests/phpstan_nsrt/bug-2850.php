<?php

namespace Bug2850;

use function PHPStan\Testing\assertType;

class Foo
{
	public function y(): void {}
}

class Bar
{
	/** @var Foo|null */
	private $x;

	public function getFoo(): Foo
	{
		if ($this->x === null) {
			$this->x = new Foo();
			assertType(Foo::class, $this->x);
			$this->x->y();
			assertType(Foo::class, $this->x);
			$this->y();
			assertType(Foo::class . '|null', $this->x);
		}
		return $this->x;
	}

	public function y(): void
	{
		$this->x = null;
	}
}
