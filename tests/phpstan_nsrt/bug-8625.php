<?php

namespace Bug8625;

use function PHPStan\Testing\assertType;

class HelloWorld
{
	public static function sayHello(): void
	{
		$key = false;
		$loopArray = [1, 2, 3];
		foreach ($loopArray as $i) {
			$key = $key === false;
			assertType('bool', $key);
			if ($key) {
				echo "i=$i key is true\n";
			} else {
				echo "i=$i key is false\n";
			}
		}
	}
}
