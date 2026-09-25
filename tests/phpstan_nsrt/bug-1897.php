<?php declare(strict_types=1);

namespace Bug1897;

use function PHPStan\Testing\assertType;

class ObjectA
{

}

class Example
{
	function foo(ObjectA $object = null): void
	{
		$object = $object ?: new ObjectA();
		assertType('Bug1897\\ObjectA', $object);
	}
}
