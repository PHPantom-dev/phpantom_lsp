<?php // lint >= 8.0

namespace Callables;

use function PHPStan\Testing\assertType;

class Foo
{

	public function doFoo(): float
	{
		$closure = function (): string {

		};
		$foo = $this;
		$arrayWithStaticMethod = ['Callables\\Foo', 'doBar'];
		$stringWithStaticMethod = 'Callables\\Foo::doFoo';
		$arrayWithInstanceMethod = [$this, 'doFoo'];
		assertType('int', $foo());
		assertType('string', $closure()); // SKIP: a closure called through ->call() or a callable held in a variable is not resolved
		assertType('*ERROR*', $arrayWithStaticMethod());
		assertType('*ERROR*', $stringWithStaticMethod());
		assertType('float', $arrayWithInstanceMethod()); // SKIP: a closure called through ->call() or a callable held in a variable is not resolved
		assertType('mixed', $closureObject());
	}

	public function doBar(): Bar
	{

	}

	public function __invoke(): int
	{

	}

}
