<?php

namespace ClosureTypes;

use DateTimeInterface;
use stdClass;
use function PHPStan\Testing\assertType;

class Foo
{

	/** @var array<int, array{foo: string, bar: int}> */
	private $arrayShapes;

	public function doFoo(): void
	{
		$a = array_map(function (array $a): array {
			assertType('array{foo: string, bar: int}', $a);

			return $a;
		}, $this->arrayShapes);
		assertType('array<int, array{foo: string, bar: int}>', $a);

		$b = array_map(function ($b) {
			assertType('array{foo: string, bar: int}', $b);

			return $b['foo'];
		}, $this->arrayShapes);
		assertType('array<int, string>', $b);
	}

	public function doBar(): void
	{
		usort($this->arrayShapes, function (array $a, array $b): int {
			assertType('array{foo: string, bar: int}', $a);
			assertType('array{foo: string, bar: int}', $b);

			return 1;
		});
	}

	public function doBaz(): void
	{
		usort($this->arrayShapes, function ($a, $b): int {
			assertType('array{foo: string, bar: int}', $a);
			assertType('array{foo: string, bar: int}', $b);

			return 1;
		});
	}

	public function closureNewThisIntersection(stdClass $foo) {
		if (!$foo instanceof DateTimeInterface) {
			return;
		}

		(function () {
			assertType('DateTimeInterface&stdClass', $this);
		})->call($foo);
	}

	public function arrowFunctionNewThisIntersection(stdClass $foo) {
		if (!$foo instanceof DateTimeInterface) {
			return;
		}

		(fn () => assertType('DateTimeInterface&stdClass', $this))->call($foo);
	}

}
