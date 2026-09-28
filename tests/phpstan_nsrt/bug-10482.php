<?php

namespace Bug10482;

use function PHPStan\Testing\assertType;

class Test
{
	public ?int $id;
}

$test = new Test();
if (rand(0, 1)) {
	$test = null;
}

$testId = $test?->id;
if (null !== $testId) {
	assertType('Bug10482\Test', $test);
}

if ($testId) {
	assertType('Bug10482\Test', $test);
}
