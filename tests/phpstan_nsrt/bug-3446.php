<?php

namespace Bug3446;

use function PHPStan\Testing\assertType;

class HelloWorld
{
	/**
	 * @param list<string> $s
	 */
	public function takesStringArray(array $s) : void{}

	public function takesString(string $s) : void{}

	public function main2(string $input) : void{
		if(is_array($var = json_decode($input))){
			assertType('array<mixed, mixed>', $var);
		}
	}
}
