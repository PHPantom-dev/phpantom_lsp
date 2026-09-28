<?php

namespace Bug3710;

use function PHPStan\Testing\assertType;

class Person {
	public function aMethod(): void {}
}

class PersonCreationFailedException extends \Exception {}

class Foo
{

	/**
	 * @throws PersonCreationFailedException
	 */
	function createPersonButCouldFail(): Person
	{
		throw new PersonCreationFailedException();
	}

	public function doFoo()
	{
		$person = null;
		try {
			$person = $this->createPersonButCouldFail();
		} finally {
			assertType('Bug3710\Person|null', $person);
		}

		assertType('Bug3710\Person', $person);
	}

}
