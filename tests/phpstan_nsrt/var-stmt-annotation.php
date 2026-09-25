<?php

namespace VarStatementAnnotation;

use function PHPStan\Testing\assertType;

class Foo
{

	/**
	 * @param object $object
	 */
	public function doFoo($object)
	{
		/** @var self $object */
		echo 'fooo';

		assertType('VarStatementAnnotation\Foo', $object);
	}

	/**
	 * @param object $object
	 */
	public function doBar($object)
	{
		/** @var self $object */
		$object->foo();

		die;
	}

	/**
	 * @param object $object
	 */
	public function doBaz($object)
	{
		/** @var self $object */
		$test = doFoo();

		die;
	}

}
