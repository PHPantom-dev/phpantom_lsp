<?php

namespace Bug6699;

use function PHPStan\Testing\assertType;

/**
 * @template TValue
 */
class Foo
{
	/** @var TValue */
	public $value;

	/**
	 * @param TValue $value
	 */
	public function __construct($value) {
		$this->value = $value;
	}

	/**
	 * @param class-string<\Exception> $exceptionClass
	 * @return void
	 */
	public function doFoo(string $exceptionClass)
	{
		assertType('class-string<Exception>', (new Foo($exceptionClass))->value);
	}
}
