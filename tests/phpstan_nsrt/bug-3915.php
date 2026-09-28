<?php

namespace Bug3915;

use function PHPStan\Testing\assertType;

class HelloWorld
{

	public function sayHello(): void
	{
		$lengths = [0];
		foreach ([1] as $row) {
			$lengths[] = self::getInt();
		}
		assertType('non-empty-list<int>', $lengths);
	}

	public static function getInt(): int
	{
		return 5;
	}

}
