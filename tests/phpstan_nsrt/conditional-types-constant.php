<?php

namespace ConditionalTypesConstant;

use function PHPStan\Testing\assertType;
use const PREG_SPLIT_NO_EMPTY;
use const PREG_SPLIT_OFFSET_CAPTURE;

abstract class Test
{
	/**
	 * @return ($flag is PREG_SPLIT_NO_EMPTY ? true : false)
	 */
	abstract public function returnsTrueForPREG_SPLIT_NO_EMPTY(int $flag): bool;

	public function test(): void
	{
		assertType('true', $this->returnsTrueForPREG_SPLIT_NO_EMPTY(PREG_SPLIT_NO_EMPTY));
		assertType('true', $this->returnsTrueForPREG_SPLIT_NO_EMPTY(1));
		assertType('false', $this->returnsTrueForPREG_SPLIT_NO_EMPTY(PREG_SPLIT_OFFSET_CAPTURE));
		assertType('false', $this->returnsTrueForPREG_SPLIT_NO_EMPTY(4));
		assertType('bool', $this->returnsTrueForPREG_SPLIT_NO_EMPTY($_GET['flag']));
	}
}
