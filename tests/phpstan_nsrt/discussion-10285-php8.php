<?php // lint >= 8.0

namespace Discussion10285Php8;

use function PHPStan\Testing\assertType;

class HelloWorld
{
	public function sayHello(): void
	{
		$socket = socket_create(AF_INET, SOCK_STREAM, 0);
		if($socket === false) return;
		$read = [$socket];
		$write = [];
		$except = null;
		socket_select($read, $write, $except, 0, 1);
		assertType('array<Socket>', $read);
		assertType('array<Socket>', $write);
		assertType('null', $except);
	}
}
