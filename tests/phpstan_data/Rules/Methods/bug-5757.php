<?php

namespace Bug5757;

use function PHPStan\Testing\assertType;

class Helper
{
	/**
	 * @template       T
	 * @phpstan-param  iterable<T> $iterable
	 * @phpstan-return iterable<array<T>>
	 */
	public static function chunk(iterable $iterable, int $chunkSize): iterable
	{
		return [];
	}
}

class Foo
{

	public function doFoo()
	{
		assertType('iterable<array<1>>', Helper::chunk([1], 3));
		assertType('iterable<array<*NEVER*>>', Helper::chunk([], 3));
	}

}
