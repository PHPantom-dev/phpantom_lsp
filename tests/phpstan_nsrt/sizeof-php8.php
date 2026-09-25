<?php // lint >= 8.0

namespace sizeof_php8;

use function PHPStan\Testing\assertType;


class Sizeof
{
	/**
	 * @param int[] $ints
	 */
	function doFoo1(array $ints): string
	{
		if (count($ints) <= 0) {
			assertType('*ERROR*', min($ints));
			assertType('*ERROR*', max($ints));
		}
	}

	/**
	 * @param int[] $ints
	 */
	function doFoo2(array $ints): string
	{
		if (sizeof($ints) <= 0) {
			assertType('*ERROR*', min($ints));
			assertType('*ERROR*', max($ints));
		}
	}

	function doFoo3(array $arr): string
	{
		if (0 != count($arr)) {
			assertType('non-empty-array', $arr);
		}
		return "";
	}

	function doFoo4(array $arr): string
	{
		if (0 != sizeof($arr)) {
			assertType('non-empty-array', $arr);
		}
		return "";
	}

	function doFoo5(array $arr): void
	{
		if ([] != $arr) {
		}
		assertType('array', $arr);
	}

	function doFoo6(array $arr): void
	{
		if ($arr != []) {
		}
		assertType('array', $arr);
	}
}
