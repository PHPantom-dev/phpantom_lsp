<?php

namespace AnonymousClassName;

use function PHPStan\Testing\assertType;

function () {
	$foo = new class () {

		/** @var Foo */
		public $fooProperty;

		/**
		 * @return Foo
		 */
		public function doFoo()
		{
			assertType('AnonymousClassName\Foo', $this->fooProperty);
			assertType('AnonymousClassName\Foo', $this->doFoo());
		}
	};

	assertType('AnonymousClassName\Foo', $foo->fooProperty);
	assertType('AnonymousClassName\Foo', $foo->doFoo());
};
