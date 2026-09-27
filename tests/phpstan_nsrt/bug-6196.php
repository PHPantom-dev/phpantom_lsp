<?php

namespace Bug6196;

use function PHPStan\Testing\assertType;

final class ErrorToExceptionHandler{
	private function __construct(){

	}

	/**
	 * @phpstan-template TReturn
	 * @phpstan-param \Closure() : TReturn $closure
	 *
	 * @phpstan-return TReturn
	 * @throws \ErrorException
	 */
	public static function trap(\Closure $closure){
		return $closure();
	}
}

function (): void {
	assertType('string|false', zlib_decode("aaaaaaa"));
	assertType('string|false', ErrorToExceptionHandler::trap(fn() => zlib_decode("aaaaaaa")));
};
