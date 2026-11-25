<?php

namespace {

	if (!function_exists('array_find')) {
		/**
		 * @param array<mixed> $array
		 * @param callable(mixed, array-key=): mixed $callback
		 * @return mixed
		 */
		function array_find(array $array, callable $callback)
		{
			foreach ($array as $key => $value) {
				if ($callback($value, $key)) { // @phpstan-ignore if.condNotBoolean
					return $value;
				}
			}

			return null;
		}
	}

}

namespace ArrayFind
{

	use function PHPStan\Testing\assertType;

	/**
	 * @param array<mixed> $array
	 * @param non-empty-array<mixed> $non_empty_array
	 * @phpstan-ignore missingType.callable
	 */
	function testMixed(array $array, array $non_empty_array, callable $callback): void
	{
		assertType('mixed', array_find($array, $callback));
		assertType('int|null', array_find($array, 'is_int')); // SKIP: array_find() does not narrow by a type-checking callback such as 'is_int'
		assertType('mixed', array_find($non_empty_array, $callback));
		assertType('int|null', array_find($non_empty_array, 'is_int')); // SKIP: array_find() does not narrow by a type-checking callback such as 'is_int'
	}

	/**
	 * @param array{1, 'foo', \DateTime} $array
	 * @phpstan-ignore missingType.callable
	 */
	function testConstant(array $array, callable $callback): void
	{
		assertType("1|'foo'|DateTime|null", array_find($array, $callback));
		assertType('1', array_find($array, 'is_int')); // SKIP: array_find() does not narrow by a type-checking callback such as 'is_int'
	}

	/**
	 * @param array<int> $array
	 * @param non-empty-array<int> $non_empty_array
	 * @phpstan-ignore missingType.callable
	 */
	function testInt(array $array, array $non_empty_array, callable $callback): void
	{
		assertType('int|null', array_find($array, $callback));
		assertType('int|null', array_find($array, 'is_int'));
		assertType('int|null', array_find($non_empty_array, $callback));
		// should be 'int'
		assertType('int|null', array_find($non_empty_array, 'is_int'));
	}

	function testCallback(): void
	{
		$subject = ['foo' => 1, 'bar' => null, 'buz' => ''];
		$result = array_find($subject, function ($value, $key) {
			assertType("array{value: 1|''|null, key: 'bar'|'buz'|'foo'}", compact('value', 'key')); // SKIP: compact() gives `array` rather than a shape of the named variables

			return is_int($value);
		});

		assertType("1|''|null", $result);
	}

}
