<?php

namespace Bug11488Nsrt;

use function PHPStan\Testing\assertType;

class Foo
{
	/**
	 * @param array{mixed}|array{mixed, string|null, mixed} $row
	 */
	protected function test(array $row): string
	{
		if (count($row) !== 1) {
			assertType('array{mixed, string|null, mixed}', $row);

			[$field, $operator, $value] = $row;
			assertType('string|null', $operator);
		} else {
			assertType('array{mixed}', $row);
		}

		if (count($row) === 1) {
			assertType('array{mixed}', $row);
		} else {
			assertType('array{mixed, string|null, mixed}', $row);
		}

		if (count($row) === 2) {
			assertType('*NEVER*', $row);
		} else {
			assertType('array{mixed, string|null, mixed}|array{mixed}', $row);
		}

		if (count($row) !== 2) {
			assertType('array{mixed, string|null, mixed}|array{mixed}', $row);
		} else {
			assertType('*NEVER*', $row);
		}

		if (count($row) === 3) {
			assertType('array{mixed, string|null, mixed}', $row);
		} else {
			assertType('array{mixed}', $row);
		}

		return '';
	}

	/**
	 * @param array{bool}|array{mixed, string|null, mixed} $row
	 */
	protected function test2(array $row): string
	{
		if (count($row) !== 1) {
			assertType('array{mixed, string|null, mixed}', $row);

			[$field, $operator, $value] = $row;
			assertType('string|null', $operator);
		} else {
			assertType('array{bool}', $row);
		}

		if (count($row) === 1) {
			assertType('array{bool}', $row);
		} else {
			assertType('array{mixed, string|null, mixed}', $row);
		}

		if (count($row) === 2) {
			assertType('*NEVER*', $row);
		} else {
			assertType('array{bool}|array{mixed, string|null, mixed}', $row);
		}

		if (count($row) !== 2) {
			assertType('array{bool}|array{mixed, string|null, mixed}', $row);
		} else {
			assertType('*NEVER*', $row);
		}

		if (count($row) === 3) {
			assertType('array{mixed, string|null, mixed}', $row);
		} else {
			assertType('array{bool}', $row);
		}

		return '';
	}
}
