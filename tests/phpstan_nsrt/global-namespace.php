<?php

namespace {

	use function PHPStan\Testing\assertType;

	/**
	 * @return Exception
	 */
	function globalNamespaceTest()
	{
		return new Exception();
	}

	function () {
		assertType('Exception', globalNamespaceTest());
	};
}
