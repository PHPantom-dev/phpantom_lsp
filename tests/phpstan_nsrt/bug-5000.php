<?php

namespace Bug5000;

use function PHPStan\Testing\assertType;

class A {

}

class B {

}

/**
 * @template T of A|B
 */
interface F
{

	/**
	 * @return T[]
	 */
	function getValues(): array;

	/**
	 * @phpstan-param T $v
	 * @return T
	 */
	function getDetail(object $v);
}

class Foo
{

	/**
	 * @template T of A|B
	 * @phpstan-param F<T> $f
	 */
	function foo(F $f): void {
		$values = $f->getValues();
		assertType('array<T of Bug5000\A|Bug5000\B (method Bug5000\Foo::foo(), argument)>', $values);

		$f->getDetail($values[0]);
		assertType('array<T of Bug5000\A|Bug5000\B (method Bug5000\Foo::foo(), argument)>', $values);
		foreach($values as $val) {
			assertType('T of Bug5000\A|Bug5000\B (method Bug5000\Foo::foo(), argument)', $val);
			$f->getDetail($val);
		}
	}


}
