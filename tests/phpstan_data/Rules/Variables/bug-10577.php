<?php

namespace Bug10577;

use function PHPStan\Testing\assertType;

class HelloWorld
{
	private const MAP = [
		'10' => 'Test1',
		'20' => 'Test2',
	];


	public function validate(string $value): void
	{
		$value = trim($value);

		if ($value === '') {
			throw new \RuntimeException();
		}

		assertType("non-empty-string", $value);
		assertType("'Test1'|'Test2'", self::MAP[$value]);

		$value = self::MAP[$value] ?? $value;

		assertType("non-empty-string", $value);
		assertType("'Test1'|'Test2'", self::MAP[$value]);

		// ...
	}

	public function validateNumericString(string $value): void
	{
		if (!is_numeric($value)) return;

		assertType("numeric-string", $value);
		assertType("'Test1'|'Test2'", self::MAP[$value]);
	}
}
