<?php

namespace PDOStatement;

use function PHPStan\Testing\assertType;

class Bar {}

class Foo
{

	public function doFoo(\PDOStatement $statement)
	{
		$bar = $statement->fetchObject(Bar::class);
		assertType('PDOStatement\Bar|false', $bar);

		$bar = $statement->fetchObject();
		assertType('stdClass|false', $bar);
	}

}

