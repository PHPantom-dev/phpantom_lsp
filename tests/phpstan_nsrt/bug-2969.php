<?php

namespace Bug2969;


use function PHPStan\Testing\assertType;

class HelloWorld
{
	public function methodWithOccasionalUndocumentedException(): void
	{
		if (time()%2==0) {
			throw new \Exception('Go straight to finally');
		}
	}

	public function execute(): void
	{
		// ...Upload image to permanent storage

		$done = false;
		try{
			// call method to save reference to database
			$this->methodWithOccasionalUndocumentedException();
			$done = true;
		} finally {
			assertType('bool', $done);
		}
	}
}
