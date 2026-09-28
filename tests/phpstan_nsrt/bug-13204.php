<?php declare(strict_types = 1);

namespace Bug13204;

use function PHPStan\Testing\assertType;

/**
 * @template TChild of object
 * @extends \ArrayAccess<int, TChild|null>
 */
interface ParentNode extends \ArrayAccess {}

class HelloWorld
{
	public function sayHelloBug(object $node): void
	{
		if ($node instanceof ParentNode) {
			assertType('object|null', $node[0]);
		}
	}
}
