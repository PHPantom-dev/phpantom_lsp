<?php

namespace Bug13985;

use SplObjectStorage;
use function PHPStan\Testing\assertType;

function example(mixed $param): void
{
	if ($param instanceof SplObjectStorage) {
		foreach ($param as $key => $value) {
			assertType('int', $key);
			assertType('object', $value);
		}
	}
}

class X {}

/**
 * @param SplObjectStorage<X, int> $splObjectStorage
 * @return void
 */
function genericExample(SplObjectStorage $splObjectStorage): void
{
	foreach ($splObjectStorage as $key => $value) {
		assertType('int', $key);
		assertType('Bug13985\X', $value);
	}
	assertType('int', $splObjectStorage->getInfo());

}
