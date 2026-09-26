<?php

namespace Bug6301;

use function PHPStan\Testing\assertType;

class HelloWorld
{
	/**
	 * @template T of string
	 * @param T $s
	 * @return T
	 */
	public function str($s)
	{
		return $s;
	}

	/**
	 * @param non-empty-string $nonEmpty
	 * @param numeric-string $numericString
	 * @param literal-string $literalString
	 */
	public function foo(int $i, $nonEmpty, $numericString, $literalString):void {
		assertType('non-empty-string', $this->str($nonEmpty));
		assertType('numeric-string', $this->str($numericString));
		assertType('literal-string', $this->str($literalString));
	}
}
