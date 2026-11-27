<?php

namespace CaseInsensitiveParent;

use function PHPStan\Testing\assertType;

class A {
	const myConst = '1';

	public function doFoo():string {
		return "hello";
	}

}

class B extends A {
	public function doFoo():string {
		assertType('string', PARENT::doFoo());
		assertType('string', parent::doFoo());

		assertType("'1'", PARENT::myConst);
		assertType("'1'", parent::myConst);

		assertType("'CaseInsensitiveParent\\\\A'", PARENT::class);

		return PARENT::doFoo();
	}
}

class C extends UnknownParent {
	public function doFoo():string {
		// PHPantom is more precise than PHPStan here: The parent class is unknown but its name is still known from the extends clause, so `parent::class` is that name, which is more useful than a bare class-string.
		assertType("'CaseInsensitiveParent\\\\UnknownParent'", PARENT::class);
	}

}
