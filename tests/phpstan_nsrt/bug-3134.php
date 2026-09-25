<?php

namespace Bug3134;

use function PHPStan\Testing\assertType;

class Registry
{
	/**
	 * Map of type names and their corresponding flyweight objects.
	 *
	 * @var array<string, object>
	 */
	private $instances = [];

	public function get(string $name) : object
	{
		return $this->instances[$name];
	}
}

function (Registry $r): void {
	assertType('bool', $r->get('x') === $r->get('x'));
};
