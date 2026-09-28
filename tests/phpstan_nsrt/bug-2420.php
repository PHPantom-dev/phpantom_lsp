<?php

namespace Bug2420;

use function PHPStan\Testing\assertType;

class HelloWorld
{
	const CONFIG = [
		0 => false,
		1 => false,
	];

	public function sayHello(int $key): void
	{
		$config = self::CONFIG[$key] ?? true;
		assertType('bool', $config);
	}
}

class HelloWorld2
{
	const CONFIG = [
		0 => ['foo' => false],
		1 => ['foo' => false],
	];

	public function sayHello(int $key): void
	{
		$config = self::CONFIG[$key]['foo'] ?? true;
		assertType('bool', $config);
	}
}
