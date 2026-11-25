<?php

namespace Bug7301;

use Closure;
use function PHPStan\Testing\assertType;

/**
 * @template TReturn
 * @param Closure(): TReturn $closure
 * @return TReturn
 */
function templated($closure)
{
	return $closure();
}

function (): void {
	/**
	 * @var Closure(): array<non-empty-string, mixed>
	 */
	$arg = function () {
		return ['key' => 'value'];
	};

	$result = templated($arg);

	assertType('array<non-empty-string, mixed>', $result);
};
