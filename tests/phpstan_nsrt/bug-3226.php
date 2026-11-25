<?php

namespace Bug3226;

use function PHPStan\Testing\assertType;

class Foo
{
	/**
	 * @var class-string
	 */
	private $class;

	/**
	 * @param class-string $class
	 */
	public function __construct(string $class)
	{
		$this->class = $class;
	}

	/**
	 * @return class-string
	 */
	public function __toString(): string
	{
		return $this->class;
	}
}

function (Foo $foo): void {
	assertType('class-string', $foo->__toString());
	assertType('class-string', (string) $foo);
};

/**
 * @template T
 */
class Bar
{
	/**
	 * @var class-string<T>
	 */
	private $class;

	/**
	 * @param class-string<T> $class
	 */
	public function __construct(string $class)
	{
		$this->class = $class;
	}

	/**
	 * @return class-string<T>
	 */
	public function __toString(): string
	{
		return $this->class;
	}
}

function (Bar $bar): void {
	assertType('class-string<mixed>', $bar->__toString());
	assertType('class-string<mixed>', (string) $bar);
};

function (): void {
	$bar = new Bar(\Exception::class);
	assertType('class-string<Exception>', $bar->__toString());
	assertType('class-string<Exception>', (string) $bar);
};
