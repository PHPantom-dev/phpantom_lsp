<?php // lint >= 8.0

namespace MatchExpr;

use function get_class;
use function PHPStan\Testing\assertType;

class Foo
{

	/**
	 * @param 1|2|3|4 $i
	 */
	public function doFoo(int $i): void
	{
		assertType('*NEVER*', match ($i) { // SKIP: a branch that cannot run keeps the type instead of `*NEVER*`
			0 => $i,
		});
		assertType('1|2|3|4', $i);
		assertType('1', match ($i) {
			1 => $i,
		});
		assertType('1|2|3|4', $i);
		assertType('1|2', match ($i) {
			1, 2 => $i,
		});
		assertType('1|2|3|4', $i);
		assertType('1|2|3', match ($i) {
			1, 2, 3 => $i,
		});
		assertType('1|2|3|4', $i);
		assertType('2|3', match ($i) {
			1 => exit(),
			2, 3 => $i,
		});
		assertType('1|2|3|4', $i);
	}

	/**
	 * @param 1|2|3|4 $i
	 */
	public function doBar(int $i): void
	{
		match ($i) {
			0 => assertType('*NEVER*', $i), // SKIP: a branch that cannot run keeps the type instead of `*NEVER*`
			default => assertType('1|2|3|4', $i),
		};
		assertType('1|2|3|4', $i);
		match ($i) {
			1 => assertType('1', $i),
			default => assertType('2|3|4', $i),
		};
		assertType('1|2|3|4', $i);
		match ($i) {
			1, 2 => assertType('1|2', $i),
			default => assertType('3|4', $i),
		};
		assertType('1|2|3|4', $i);
		match ($i) {
			1, 2, 3 => assertType('1|2|3', $i),
			default => assertType('4', $i),
		};
		assertType('1|2|3|4', $i);

		match ($i) {
			assertType('1|2|3|4', $i), 1, assertType('2|3|4', $i) => null, // SKIP: an assignment inside a match arm's condition is not walked
			assertType('2|3|4', $i) => null, // SKIP: an assignment inside a match arm's condition is not walked
			default => assertType('2|3|4', $i),
		};
	}

	public function doGettype(int|float|bool|string|object|array $value): void
	{
		match (gettype($value)) {
			'integer' => assertType('int', $value),
			'string' => assertType('string', $value),
		};
	}

	public function doGettypeUnion(int|float|bool|string|object|array $value): void
	{
		$intOrString = 'integer';
		if (rand(0, 1)) {
			$intOrString = 'string';
		}
		match (gettype($value)) {
			$intOrString => assertType('int|string', $value), // SKIP: a match arm whose condition is a variable holding literals does not narrow the gettype() subject
		};
	}

}

final class FinalFoo
{

}

final class FinalBar
{

}

class TestGetClass
{

	public function doMatch(FinalFoo|FinalBar $class): void
	{
		match (get_class($class)) {
			FinalFoo::class => assertType(FinalFoo::class, $class),
			FinalBar::class => assertType(FinalBar::class, $class),
		};

		match (get_debug_type($class)) {
			FinalFoo::class => assertType(FinalFoo::class, $class), // SKIP: get_debug_type() in a match subject does not narrow
			FinalBar::class => assertType(FinalBar::class, $class), // SKIP: get_debug_type() in a match subject does not narrow
		};
	}

}
