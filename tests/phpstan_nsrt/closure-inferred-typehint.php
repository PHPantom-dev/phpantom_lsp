<?php

namespace ClosureWithInferredTypehint;

use function PHPStan\Testing\assertType;

class Foo
{

	public function doFoo()
	{
		$this->doBar(function ($foo, $bar) {
			assertType('DateTime|stdClass', $foo);
			assertType('mixed', $bar);
		});
		$this->doBaz(function ($foo, $bar) {
			die;
		});
	}

	/**
	 * @param \Closure(\DateTime|\stdClass): void $closure
	 */
	private function doBar(\Closure $closure)
	{

	}

	/**
	 * @param callable(\DateTime|\stdClass): void $closure
	 */
	private function doBaz(callable $closure)
	{

	}

}
