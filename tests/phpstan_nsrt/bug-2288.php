<?php

namespace Bug2288;

use function PHPStan\Testing\assertType;

class One
{
	public function test() :?\DateTimeImmutable
	{
		return rand(0,1) === 1 ? new \DateTimeImmutable(): null;
	}
}

class Two
{
	public function test(): void
	{
		$test = new One();
		if ($test->test()) {
			assertType(\DateTimeImmutable::class, $test->test());
		}
	}
}
