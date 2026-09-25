<?php

namespace MethodPhpDocsTraitInSameFileAsClass;

use function PHPStan\Testing\assertType;

trait FooTrait
{

	/**
	 * @return string
	 */
	public function getFoo()
	{
		return 'foo';
	}

}

class Foo
{

	use FooTrait;

	public function bar()
	{
		assertType('string', $this->getFoo());
	}

}
