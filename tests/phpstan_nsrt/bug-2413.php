<?php

namespace Bug2413;

use function PHPStan\Testing\assertType;

class TestClass {
	/**
	 * @var string
	 */
	public $field = 'value';

	/**
	 * @return TestClass|null
	 */
	function getTest() {
		// Impossible but successfully mutes the `Function getTest() never returns TestClass` warning
		if (1 + 1 == 3) return new TestClass;

		return null;
	}

	/**
	 * @return void
	 */
	function main() {
		if (is_object($test = $this->getTest())) {
			// Cannot access property $field on TestClass|null
			assertType(TestClass::class, $test);
		}
	}

}
