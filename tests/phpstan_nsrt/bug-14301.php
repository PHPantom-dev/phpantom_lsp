<?php declare(strict_types = 1);

namespace Bug14301;

use function PHPStan\Testing\assertType;

class Foo
{
	/**
	 * @param array{bool}|array{mixed, string|null, mixed} $row
	 */
	protected function testNotEquals(array $row): string
	{
		if (count($row) !== 1) {
			assertType('array{mixed, string|null, mixed}', $row);

			[$field, $operator, $value] = $row;
			assertType('string|null', $operator);
			return $operator ?? '=';
		} else {
			assertType('array{bool}', $row);
		}

		return '';
	}

	/**
	 * @param array{bool}|array{mixed, string|null, mixed} $row
	 */
	protected function testEquals(array $row): void
	{
		if (count($row) === 3) {
			assertType('array{mixed, string|null, mixed}', $row);
		} else {
			assertType('array{bool}', $row);
		}
	}

	/**
	 * @param array{bool}|array{mixed, string|null, mixed} $row
	 */
	protected function testEquals1(array $row): void
	{
		if (count($row) === 1) {
			assertType('array{bool}', $row);
		} else {
			assertType('array{mixed, string|null, mixed}', $row);
		}
	}
}
