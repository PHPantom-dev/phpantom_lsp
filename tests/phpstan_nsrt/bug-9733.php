<?php declare(strict_types = 1);

namespace Bug9733;

use function PHPStan\Testing\assertType;

abstract class Base{
	/**
	 * @param int[] $array
	 */
	abstract public function test(array $array) : void;
}

trait MyTrait{
	public function test(array $array) : void{
		assertType('array<int>', $array);
	}
}

class Concrete extends Base{
	use MyTrait {
		test as test2;
	}
}
