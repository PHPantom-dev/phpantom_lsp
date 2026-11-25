<?php // lint >= 8.0

namespace AssertInheritance;

use function PHPStan\Testing\assertType;

/**
 * @template T
 */
interface WrapperInterface
{
	/**
	 * @phpstan-assert T $param
	 */
	public function assert(mixed $param): void;

	/**
	 * @phpstan-assert-if-true T $param
	 */
	public function supports(mixed $param): bool;

	/**
	 * @phpstan-assert-if-false T $param
	 */
	public function notSupports(mixed $param): bool;
}

/**
 * @implements WrapperInterface<int>
 */
class IntWrapper implements WrapperInterface
{
	public function assert(mixed $param): void
	{
	}

	public function supports(mixed $param): bool
	{
		return is_int($param);
	}

	public function notSupports(mixed $param): bool
	{
		return !is_int($param);
	}
}

/**
 * @template T of object
 * @implements WrapperInterface<T>
 */
abstract class ObjectWrapper implements WrapperInterface
{
}

/**
 * @extends ObjectWrapper<\DateTimeInterface>
 */
class DateTimeInterfaceWrapper extends ObjectWrapper
{
	public function assert(mixed $param): void
	{
	}

	public function supports(mixed $param): bool
	{
		return $param instanceof \DateTimeInterface;
	}

	public function notSupports(mixed $param): bool
	{
		return !$param instanceof \DateTimeInterface;
	}
}

function (IntWrapper $test, $val) {
	if ($test->supports($val)) {
		assertType('int', $val);
	} else {
	}

	if ($test->notSupports($val)) {
	} else {
		assertType('int', $val);
	}

	assertType('mixed', $val);
	$test->assert($val);
	assertType('int', $val); // SKIP: an inherited `@phpstan-assert` on a template leaves the variable without a type
};

function (DateTimeInterfaceWrapper $test, $val) {
	if ($test->supports($val)) {
		assertType('DateTimeInterface', $val);
	} else {
	}

	if ($test->notSupports($val)) {
	} else {
		assertType('DateTimeInterface', $val);
	}

	assertType('mixed', $val);
	$test->assert($val);
	assertType('DateTimeInterface', $val);
};
