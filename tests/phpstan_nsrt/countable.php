<?php

namespace CountablePhpDocInheritance;

use function PHPStan\Testing\assertType;

class Foo implements \Countable {
	public function count() : int {
		return 0;
	}

	static public function doFoo() {
		$foo = new Foo();
		assertType('int<0, max>', $foo->count());
	}
}

class Bar implements \Countable {

	/**
	 * @return -1
	 */
	public function count() : int {
		return -1;
	}

	static public function doBar() {
		$bar = new Bar();
		assertType('-1', $bar->count());
	}
}

interface Baz {
}

class NonCountable {}

function doNonCountable() {
}

function doFoo() {
	assertType('int<0, max>', count(new Foo()));
}

function doBar() {
}

function doBaz(Baz $baz) {
	assertType('int<0, max>', count($baz));
}
