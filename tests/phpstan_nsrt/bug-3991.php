<?php

namespace Bug3991;

use function PHPStan\Testing\assertNativeType;
use function PHPStan\Testing\assertType;

class Foo
{
	/**
	 * @param \stdClass|array|null $config
	 *
	 * @return \stdClass
	 */
	public static function email($config = null)
	{
		assertNativeType('mixed', $config);
		assertType('array|stdClass|null', $config);
		if (empty($config))
		{
			// the native type should be `0|0.0|''|'0'|array{}|false|null`
			// the problem is that `empty($config)` translates to `!isset($config) || !$config`
			// and before specified types of the left and right side are intersected they are "normalized"
			// by removing the sureNotType from $scope->getType() which is `stdClass|array|null` here
			assertNativeType('array{}|null', $config);
			assertType('array{}|null', $config);
			$config = new \stdClass();
		} elseif (! (is_array($config) || $config instanceof \stdClass)) {
			assertNativeType('mixed~(0|0.0|\'\'|\'0\'|array{}|stdClass|false|null)', $config);
			assertType('*NEVER*', $config);
		}

		return new \stdClass($config);
	}
}
