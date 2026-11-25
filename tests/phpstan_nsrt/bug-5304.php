<?php

namespace Bug5304;

use function PHPStan\Testing\assertType;

/**
 * @param array{foo?: string} $in
 */
function takesArg(array $in): void
{
	if(is_string($in['foo'] ?? null)) {
		assertType('array{foo: string}', $in);
	}

	if(isset($in['foo'])) {
		assertType('array{foo: string}', $in);
	}

}
