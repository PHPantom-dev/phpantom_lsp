<?php

namespace Trim;

use function PHPStan\Testing\assertType;

class Foo
{

	/**
	 * @param 'foo' $foo
	 * @param 'foo'|'bar' $fooOrBar
	 * @param 'constant'|class-string<Foo> $constantOrFooClass
	 * @param string $string
	 */
	public function doTrim($foo, $fooOrBar, $constantOrFooClass, $string): void
	{
		assertType('string', trim($string, $foo));
		assertType('string', ltrim($string, $foo));
		assertType('string', rtrim($string, $foo));

		assertType('\'\'|\'foo\'', trim($foo, $fooOrBar)); // SKIP: a string function or `++` on literals is not folded
		assertType('\'\'|\'foo\'', ltrim($foo, $fooOrBar)); // SKIP: a string function or `++` on literals is not folded
		assertType('\'\'|\'foo\'', rtrim($foo, $fooOrBar)); // SKIP: a string function or `++` on literals is not folded

		assertType('\'\'|\'bar\'', trim($fooOrBar, $foo)); // SKIP: a string function or `++` on literals is not folded
		assertType('\'\'|\'bar\'', ltrim($fooOrBar, $foo)); // SKIP: a string function or `++` on literals is not folded
		assertType('\'\'|\'bar\'', rtrim($fooOrBar, $foo)); // SKIP: a string function or `++` on literals is not folded

		assertType('string', trim($constantOrFooClass, '\\'));
		assertType('string', ltrim($constantOrFooClass, '\\'));
		assertType('string', rtrim($constantOrFooClass, '\\'));
		assertType('string', trim($constantOrFooClass, '\\'));
		assertType('string', ltrim($constantOrFooClass, '\\'));
		assertType('string', rtrim($constantOrFooClass, '\\'));
	}

}
