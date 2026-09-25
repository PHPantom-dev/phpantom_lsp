<?php

use function PHPStan\Testing\assertType;

use AnotherNamespace\Foo;

/** @var Foo[][] $fooses */
$fooses = foos();

foreach ($fooses as $foos) {
	foreach ($foos as $foo) {
		assertType('AnotherNamespace\Foo', $foo);
		assertType('AnotherNamespace\Foo', $foos[0]);
		assertType('AnotherNamespace\Foo', $fooses[0][0]);
	}
}
