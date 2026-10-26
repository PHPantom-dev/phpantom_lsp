<?php

namespace Bug4587;

use function PHPStan\Testing\assertType;

class HelloWorld
{
	public function a(): void
	{
		/** @var list<array{a: int}> $results */
		$results = [];

		$type = array_map(static function (array $result): array {
			assertType('array{a: int}', $result);
			return $result;
		}, $results);

		assertType('list<array{a: int}>', $type);
	}

	public function b(): void
	{
		/** @var list<array{a: int}> $results */
		$results = [];

		$type = array_map(static function (array $result): array {
			assertType('array{a: int}', $result);
			$result['a'] = (string) $result['a'];
			// PHPantom does not track PHPStan's lowercase-string and uppercase-string accessories here.
			assertType('array{a: numeric-string}', $result);

			return $result;
		}, $results);

		// PHPantom does not track PHPStan's lowercase-string and uppercase-string accessories here.
		assertType('list<array{a: numeric-string}>', $type);
	}
}
