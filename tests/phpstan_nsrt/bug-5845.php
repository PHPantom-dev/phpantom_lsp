<?php

namespace Bug5845;

use function PHPStan\Testing\assertType;

class A
{
	/**
	 * @param resource $resource
	 */
	public static function foo($resource): void
	{
		assertType('bool', is_resource($resource));
		$type = is_resource($resource) ? get_resource_type($resource) : 'closed';
	}
}
