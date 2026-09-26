<?php

namespace Bug5749;

use function PHPStan\Testing\assertType;

abstract class ActiveRowFactory
{
	/**
	 * @param int[] $array
	 *
	 * @return string
	 */
	public static function dbarray_int($array) {
		// ...
		return '';
	}
}

class FooFactory extends ActiveRowFactory {

	/**
	 * @param null|int|int[] $type
	 * @return string
	 */
	public function getTypeSql($type = null) {

		if (
			$type
			&&
			!is_array($type)
		) {
			$type = [$type];
		}

		assertType('0|array<int>|null', $type); // SKIP: falsiness narrowing does not narrow to the falsy values

		if ($type) {
			$typeSql = ' AND type IN ' . self::dbarray_int($type) . ' ';
		} else {
			assertType('0|array{}|null', $type); // SKIP: falsiness narrowing does not narrow to the falsy values
			$typeSql = '';
		}

		// ...

		return $typeSql;
	}
}
