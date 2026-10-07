<?php

namespace ResolveStatic;

use function PHPStan\Testing\assertType;

class Foo
{

	/**
	 * @return static
	 */
	public static function create()
	{
		return new static();
	}

	/**
	 * @return array{foo: static}
	 */
	public function returnConstantArray(): array
	{
		return [$this];
	}

	/**
	 * @return static
	 */
	public function nullabilityNotInSync(): ?self
	{

	}

	/**
	 * @return static|null
	 */
	public function anotherNullabilityNotInSync(): self
	{

	}

}

class Bar extends Foo
{

}

function (Bar $bar) {
	assertType('ResolveStatic\Foo', \ResolveStatic\Foo::create());
	assertType('ResolveStatic\Bar', \ResolveStatic\Bar::create());
	assertType('array{foo: ResolveStatic\Bar}', $bar->returnConstantArray());
	assertType('ResolveStatic\Bar|null', $bar->nullabilityNotInSync());
	assertType('ResolveStatic\Bar', $bar->anotherNullabilityNotInSync());
};
