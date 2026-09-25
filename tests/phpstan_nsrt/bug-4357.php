<?php

namespace Bug4357;

use function PHPStan\Testing\assertType;

class Sample {
	/** @var null|array<string, string> */
	private $arr = null;

	public function test(): void {
		if ($this->arr === null) {
			return;
		}

		assertType('array<string, string>', $this->arr);

		unset($this->arr['hello']);

		assertType('array<string, string>', $this->arr);

		if (count($this->arr) === 0) {
			$this->arr = null;
		}
	}
}
