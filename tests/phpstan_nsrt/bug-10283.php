<?php declare(strict_types = 1);

namespace Bug10283;

use function PHPStan\Testing\assertType;

class JsExpressionable {}

class Cl
{
    /**
     * @param \Closure(): JsExpressionable|\Closure(): int $fx
     */
	public function test($fx): ?JsExpressionable
	{
		$res = $fx();
		assertType('Bug10283\JsExpressionable|int', $res);

		if (is_int($res)) {
			return null;
		}

		return $res;
	}
}
