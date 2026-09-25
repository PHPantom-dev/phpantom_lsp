<?php

use function PHPStan\Testing\assertType;

use AnotherNamespace\Foo;

/** @var int[] $integers */
$integers = foos();

foreach ($integers as $integer) {
	assertType('int', $integer);
}
