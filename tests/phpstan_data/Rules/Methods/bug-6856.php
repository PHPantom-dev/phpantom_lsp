<?php

namespace Bug6856;

use function PHPStan\Testing\assertType;

/**
 * @template T of object
 */
trait TraitA {
	/**
	 * @return T
	 */
	public function a(): object {
		return new ClassB();
	}

	/**
	 * @return T
	 */
	public function test(): object {
		return new ClassB();
	}
}

class ClassA {
	/**
	 * @phpstan-use TraitA<ClassB>
	 */
	use TraitA {
		a as renamed;
	}

	public function a(): ClassB {
		return $this->renamed();
	}

	public function b(): ClassB {
		return $this->test();
	}
}

class ClassB {
	// empty
}

function (ClassA $a): void {
	assertType(ClassB::class, $a->a());
	assertType(ClassB::class, $a->renamed());
	assertType(ClassB::class, $a->test());
	assertType(ClassB::class, $a->b());
};
