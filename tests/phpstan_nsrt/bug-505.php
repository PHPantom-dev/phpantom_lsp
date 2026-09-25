<?php

namespace Bug505;

use function PHPStan\Testing\assertType;

class Test
{
	public function foo(int $x): string
	{
		try {
			$x = $this->resolve($x);
		} catch (\Throwable $e) {
			assertType('int', $x);
		}

		return 'bar';
	}

	private function resolve(int $x) : string
	{
		return 'ok';
	}

	private function handleError(int $x) : string
	{
		return 'error';
	}
}
