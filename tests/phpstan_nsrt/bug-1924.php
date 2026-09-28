<?php

namespace Bug1924;

use function PHPStan\Testing\assertType;

class Bug1924
{

	function getArrayOrNull(): ?array
	{
		return rand(0, 1) ? [1, 2, 3] : null;
	}

	function foo(): void
	{
		$arr = [
			'a' => $this->getArrayOrNull(),
			'b' => $this->getArrayOrNull(),
		];
		assertType('array{a: array|null, b: array|null}', $arr);

		$cond = isset($arr['a']) && isset($arr['b']);
		assertType('bool', $cond);
	}

}
