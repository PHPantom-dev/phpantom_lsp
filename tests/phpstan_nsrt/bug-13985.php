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
		assertType('int', $key); // SKIP: foreach over SplObjectStorage swaps its keys and values
		assertType('Bug13985\X', $value); // SKIP: foreach over SplObjectStorage swaps its keys and values
	}
	assertType('int', $splObjectStorage->getInfo());

}
