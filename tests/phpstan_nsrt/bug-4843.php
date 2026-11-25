<?php

namespace Bug4843;

use function PHPStan\Testing\assertType;

class Bar {
	/**
	 * @var int
	 * @psalm-var 0|positive-int
	 */
	protected $depth = 0;

	function foo(bool $isRoot): void {
		assertType('int<0, max>', $this->depth + ($isRoot ? 0 : 1));
	}
}
