<?php declare(strict_types = 1);

namespace Base64Decode;

use function PHPStan\Testing\assertType;

class Foo
{

	public function nonStrictMode(string $string): void
	{
		assertType('string', base64_decode($string));
		assertType('string', base64_decode($string, false));
	}

	public function strictMode(string $string): void
	{
		assertType('string|false', base64_decode($string, true));
	}

}
