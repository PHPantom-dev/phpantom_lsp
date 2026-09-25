<?php // lint >= 8.0

namespace Bug12182;

use ArrayObject;
use function PHPStan\Testing\assertType;

/**
 * @extends ArrayObject<int, string>
 */
class HelloWorld extends ArrayObject
{
	public function __construct(private int $a = 42) {
	}
}

function (HelloWorld $hw): void {
	assertType('array', (array) $hw);
};
