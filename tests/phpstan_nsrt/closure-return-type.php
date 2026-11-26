<?php

namespace ClosureReturnType;

use function PHPStan\Testing\assertType;

class Foo
{

	public function doFoo(int $i): void
	{
		$f = function () {

		};
		assertType('void', $f()); // SKIP the runner reads the call through a variable, and a variable assigned a void call holds null
		assertType('Closure(): void', $f);

		$f = function () {
			return;
		};
		assertType('void', $f()); // SKIP the runner reads the call through a variable, and a variable assigned a void call holds null
		assertType('Closure(): void', $f);

		$f = function () {
			return 1;
		};
		assertType('1', $f());

		$f = function (): array {
			return ['foo' => 'bar'];
		};
		assertType('array{foo: \'bar\'}', $f());

		$f = function (string $s) {
			return $s;
		};
		assertType('string', $f('foo'));

		$f = function () use ($i) {
			return $i;
		};
		assertType('int', $f());

		$f = function () use ($i) {
			if (rand(0, 1)) {
				return $i;
			}

			return null;
		};
		assertType('int|null', $f());

		$f = function () use ($i) {
			if (rand(0, 1)) {
				return $i;
			}

			return;
		};
		assertType('int|null', $f());

		$f = function () {
			yield 1;
			return 2;
		};
		assertType('Generator<int, 1, mixed, 2>', $f());

		$g = function () use ($f) {
			yield from $f();
		};
		assertType('Generator<int, 1, mixed, void>', $g());

		$h = function (): \Generator {
			yield 1;
			return 2;
		};
		assertType('Generator<int, 1, mixed, 2>', $h());
	}

	public function doBar(): void
	{
		$f = function () {
			if (rand(0, 1)) {
				return 1;
			}

			function () {
				return 'foo';
			};

			$c = new class() {
				public function doFoo() {
					return 2.0;
				}
			};

			return 2;
		};

		assertType('1|2', $f());
	}

	/**
	 * @return never
	 */
	public function returnNever(): void
	{

	}

	public function doBaz(): void
	{
		$f = function() {
			$this->returnNever();
		};
		assertType('never', $f());

		$f = function(): void {
			$this->returnNever();
		};
		assertType('never', $f());

		$f = function() {
			if (rand(0, 1)) {
				return;
			}

			$this->returnNever();
		};
		assertType('void', $f()); // SKIP the runner reads the call through a variable, and a variable assigned a void call holds null
		assertType('Closure(): void', $f);

		$f = function(array $a) {
			foreach ($a as $v) {
				continue;
			}

			$this->returnNever();
		};
		assertType('never', $f([]));

		$f = function(array $a) {
			foreach ($a as $v) {
				$this->returnNever();
			}
		};
		assertType('void', $f([])); // SKIP the runner reads the call through a variable, and a variable assigned a void call holds null
		assertType('Closure(array): void', $f);

		$f = function() {
			foreach ([1, 2, 3] as $v) {
				$this->returnNever();
			}
		};
		assertType('never', $f());

		$f = function (): \stdClass {
			throw new \Exception();
		};
		assertType('never', $f());
	}

}
