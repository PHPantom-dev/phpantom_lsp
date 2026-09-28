<?php

namespace Bug3106;

use function PHPStan\Testing\assertType;

class Foo
{
	/** @var string|null */
	protected $expression;

	/**
	 * @return array<int,string>|null
	 */
	public function match(string $test): ?array
	{
		if($test === '') {
			return null;
		} else {
			return [1 => $test];
		}
	}

	public function test(): void
	{
		if(
			(is_string($this->expression) === true) &&
			(is_array($match = $this->match($this->expression)) === true)
		) {
			assertType('array<int, string>', $match);
		}
	}
}
