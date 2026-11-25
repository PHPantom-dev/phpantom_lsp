<?php declare(strict_types = 1);

namespace Bug10952b;

use function PHPStan\Testing\assertType;

class HelloWorld
{
	public function getString(): string
	{
		return 'hallo';
	}

	public function test(): void
	{
		$string = $this->getString();

		if (1 < mb_strlen($string)) {
		} else {
			assertType("string", $string);
		}

		if (mb_strlen($string) > 1) {
		} else {
			assertType("string", $string);
		}

		if (2 < mb_strlen($string)) {
		} else {
			assertType("string", $string);
		}

		match (true) {
			mb_strlen($string) > 0 => assertType('non-empty-string', $string),
			default => assertType("''", $string),
		};

	}

	public function getBool(): bool
	{
		return rand(0, 1) === 1;
	}

}
