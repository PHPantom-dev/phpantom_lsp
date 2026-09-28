<?php

namespace Bug5372Two;

use function PHPStan\Testing\assertType;

class X
{
	/** @var array<non-empty-string, string> */
	private $map = [];

	/**
	 * @param array<non-empty-string> $values
	 */
	public function __construct(array $values)
	{
		assertType('array<non-empty-string>', $values);
		foreach ($values as $v) {
			assertType('non-empty-string', $v);
			$this->map[$v] = 'whatever';
		}
	}
}
