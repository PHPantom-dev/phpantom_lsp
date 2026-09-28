<?php

namespace Bug7210;

use function PHPStan\Testing\assertType;

class HelloWorld {
	public function doStuff(): void {
		if (self::getBool()) {
			return;
		}

		assertType('bool', self::getBool());
	}

	/** @phpstan-impure */
	private static function getBool(): bool {
		return mt_rand(0, 1) === 0;
	}
}

class HelloWorld2 {
	public function doStuff(): void {
		if (self::getBool()) {
			return;
		}

		assertType('false', self::getBool());
	}

	private static function getBool(): bool {
		return mt_rand(0, 1) === 0;
	}
}
