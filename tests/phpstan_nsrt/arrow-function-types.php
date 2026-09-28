<?php

namespace ArrowFunctionTypes;

use function PHPStan\Testing\assertType;

class Foo
{

	/** @var array<int, array{foo: string, bar: int}> */
	private $arrayShapes;

	public function doFoo(): void
	{
		array_map(fn(array $a): array => assertType('array{foo: string, bar: int}', $a), $this->arrayShapes);
		$a = array_map(fn(array $a) => $a, $this->arrayShapes);
		assertType('array<int, array{foo: string, bar: int}>', $a);

		array_map(fn($b) => assertType('array{foo: string, bar: int}', $b), $this->arrayShapes);
		$b = array_map(fn($b) => $b['foo'], $this->arrayShapes);
		assertType('array<int, string>', $b);
	}

	public function doBar(): void
	{
		usort($this->arrayShapes, fn(array $a, array $b): int => assertType('array{foo: string, bar: int}', $a));
	}

	public function doBar2(): void
	{
		usort($this->arrayShapes, fn (array $a, array $b): int => assertType('array{foo: string, bar: int}', $b));
	}

	public function doBaz(): void
	{
		usort($this->arrayShapes, fn ($a, $b): int => assertType('array{foo: string, bar: int}', $a));
	}

	public function doBaz2(): void
	{
		usort($this->arrayShapes, fn ($a, $b): int => assertType('array{foo: string, bar: int}', $b));
	}

}
