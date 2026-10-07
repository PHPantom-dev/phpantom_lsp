<?php declare(strict_types=1);

namespace Bug12065;

use function PHPStan\Testing\assertType;

final class Foo
{
	public function bar(
		string $key,
		bool $preserveKeys,
		bool $flatten = true,
	): void {
		$format = $preserveKeys ? '[%s]' : '';

		if ($flatten) {
			$_key = sprintf($format, $key);
		} else {
			$_key = sprintf($format, $key);
			assertType('string', $_key);
			// @phpstan-ignore identical.alwaysFalse
			if ($_key === '') {
				// ...
			}
		}
	}

	/**
	 * @param int<1,2> $key
	 * @param bool     $preserveKeys
	 *
	 * @return void
	 */
	public function bar2(
		string $key,
		bool $preserveKeys,
	): void {
		$format = $preserveKeys ? '%s' : '%d';

		$_key = sprintf($format, $key);
		assertType("string", $_key);
	}
}
