<?php

namespace Bug13069;

use function PHPStan\Testing\assertType;

interface ResourceInterface {}

class Person implements ResourceInterface
{
	public function getName(): string { return 'Name'; }
}

class Account implements ResourceInterface
{
	public function getMail(): string  { return 'Mail'; }
}

function foo(?ResourceInterface $object = null): void
{
	switch ($object::class) {
		case Person::class:
			assertType(Person::class, $object);
			echo $object->getName();
			break;
		case Account::class:
			assertType(Account::class, $object);
			echo $object->getMail();
			break;
	}
}
