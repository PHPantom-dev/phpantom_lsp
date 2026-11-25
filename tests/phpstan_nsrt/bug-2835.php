<?php

namespace Bug2835AssertTypes;

use function PHPStan\Testing\assertType;

class Foo
{

	/**
	 * @param array<int, array> $tokens
	 * @return bool
	 */
	public function doFoo(array $tokens): bool {
		$i = 0;
		while (isset($tokens[$i])) {
			if ($tokens[$i]['code'] !== 1) {
				$i++;
				assertType('mixed', $tokens[$i]['code']);
				continue;
			}
			assertType('1', $tokens[$i]['code']);
			$i++;
			assertType('mixed', $tokens[$i]['code']);
			if ($tokens[$i]['code'] !== 2) {
				$i++;
				continue;
			}
			assertType('2', $tokens[$i]['code']);
			return true;
		}
		return false;
	}

}
