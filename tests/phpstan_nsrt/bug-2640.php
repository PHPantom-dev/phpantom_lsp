<?php

namespace Bug2640;

use function PHPStan\Testing\assertType;

class HelloWorld
{
	public function sayHello(): ?string
	{
		return 'Hello';
	}

	public function test(): void{
		if(!is_null($hello = $this->sayHello())){
			assertType('string', $hello);
		}
	}
}
