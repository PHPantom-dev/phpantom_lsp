<?php

namespace Bug4970;

use function PHPStan\Testing\assertType;

class Test
{
	/**
	 * @var string
	 */
	protected $importFile = '/a';

	protected function beforeRun(): int
	{
		if (!\file_exists($this->importFile)) {
			return 1;
		}
		assertType('true', \file_exists($this->importFile));
		$this->importFile = '/b';
		assertType('bool', \file_exists($this->importFile));

		if (\file_exists($this->importFile)) {
			echo 'test';
		}

		return \file_exists($this->importFile) ? 0 : 1;
	}
}
