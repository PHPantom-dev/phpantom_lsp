<?php

namespace Bug3276;

use function PHPStan\Testing\assertType;

class Foo
{

	/**
	 * @param array{name?:string} $settings
	 */
	public function doFoo(array $settings): void
	{
		$settings['name'] ??= 'unknown';
		assertType('array{name: string}', $settings);
	}

	/**
	 * @param array{name?:string} $settings
	 */
	public function doBar(array $settings): void
	{
		$settings['name'] = 'unknown';
		assertType('array{name: \'unknown\'}', $settings);
	}

}
