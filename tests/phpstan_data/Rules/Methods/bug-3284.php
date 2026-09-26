<?php

namespace Bug3284;

use function PHPStan\Testing\assertType;

class HelloWorld
{
	/**
	 * @param array{a?:string, b?:string, a?:string} $settings
	 */
	public function sayHello(array $settings): void
	{
		assertType('array{a?: string, b?: string}', $settings);
		echo 'Hello' . ($settings['b'] ?? 'unknown');
	}

	public function doFoo()
	{
		$this->sayHello(['b' => 'name']);
	}
}
