<?php // lint >= 8.1

namespace InvalidateReadonlyProperties;

use function PHPStan\Testing\assertType;

class Foo
{

	private readonly int $foo;

	public function __construct(int $foo)
	{
		$this->foo = $foo;
	}

	public function doFoo(): void
	{
		if ($this->foo === 1) {
			assertType('1', $this->foo);
			$this->doBar();
			assertType('1', $this->foo);
		}
	}

	public function doBar(): void
	{

	}

}
