<?php // lint >= 8.0

declare(strict_types = 1);

namespace Bug6991;

use function PHPStan\Testing\assertType;

class HelloWorld
{
	public int $more = 1;

	public ?string $key = null;
}

class Other {
	public ?HelloWorld $optional = null;
}

function test(Other $object): int
{
	$key = $object->optional?->key;

	if (!$key) {
		return 0;
	}

	assertType('Bug6991\HelloWorld', $object->optional);

	return $object->optional->more * 100;
}

function testNullable(null|Other $object): int
{
	$key = $object?->optional?->key;

	if (!$key) {
		return 0;
	}

	assertType('Bug6991\Other', $object);
	assertType('Bug6991\HelloWorld', $object->optional);

	return $object->optional->more * 100;
}
