<?php declare(strict_types = 1);

namespace ImplodeFunctionReturn;

use function PHPStan\Testing\assertType;

class Foo
{
	/**
	 * @param array<int> $arr
	 */
	public function ints(array $arr, int $i)
	{
		assertType("lowercase-string&uppercase-string", implode($arr));
		assertType("lowercase-string&non-empty-string&uppercase-string", implode([$i, $i]));
		if ($i !== 0) {
			assertType("lowercase-string&non-falsy-string&uppercase-string", implode([$i, $i]));
		}
	}

	const X = 'x';
	const ONE = 1;

	public function constants() {
		assertType("'12345'", implode(['12', '345']));

		assertType("'12345'", implode('', ['12', '345']));
		assertType("'12345'", join('', ['12', '345']));

		assertType("'12,345'", implode(',', ['12', '345']));
		assertType("'12,345'", join(',', ['12', '345']));

		assertType("'x,345'", join(',', [self::X, '345']));
		assertType("'1,345'", join(',', [self::ONE, '345']));
	}

	/** @param array{0: 1|2, 1: 'a'|'b'} $constArr */
	public function constArrays($constArr) {
		assertType("'1a'|'1b'|'2a'|'2b'", implode('', $constArr));
	}

	/** @param array{0: 1|2|3, 1: 'a'|'b'|'c'} $constArr */
	public function constArrays2($constArr) {
		assertType("'1a'|'1b'|'1c'|'2a'|'2b'|'2c'|'3a'|'3b'|'3c'", implode('', $constArr));
	}

	/** @param array{0: 1, 1: 'a'|'b', 2: 'x'|'y'} $constArr */
	public function constArrays3($constArr) {
		assertType("'1ax'|'1ay'|'1bx'|'1by'", implode('', $constArr));
	}

	/** @param array{0: 1, 1: 'a'|'b', 2?: 'x'|'y'} $constArr */
	public function constArrays4($constArr) {
		assertType("'1a'|'1ax'|'1ay'|'1b'|'1bx'|'1by'", implode('', $constArr));
	}

	/** @param array{10: 1|2|3, xy: 'a'|'b'|'c'} $constArr */
	public function constArrays5($constArr) {
		assertType("'1a'|'1b'|'1c'|'2a'|'2b'|'2c'|'3a'|'3b'|'3c'", implode('', $constArr));
	}

	/** @param array{0: 1, 1: 'a'|'b', 3?: 'c'|'d', 4?: 'e'|'f', 5?: 'g'|'h', 6?: 'x'|'y'} $constArr */
	public function constArrays6($constArr) {
		assertType("literal-string&lowercase-string&non-falsy-string", implode('', $constArr));
	}

	/** @param array{10: 1|2|bool, xy: 'a'|'b'|'c'} $constArr */
	public function constArrays7($constArr) {
		assertType("'1a'|'1b'|'1c'|'2a'|'2b'|'2c'|'a'|'b'|'c'", implode('', $constArr));
	}
}
