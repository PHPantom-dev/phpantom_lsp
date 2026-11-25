<?php

namespace Bug8087;

use function PHPStan\Testing\assertType;

class HelloWorld
{
	/**
	 * @param array<string, mixed> $data
	 **/
	public function sayHello(array $data): bool
	{
		\PHPStan\dumpType($data);
		assertType('array<string, mixed>', $data);

		$data['uses']   = [''];


		$data['uses'][] = '';


		return count($data['foo']) > 0;
	}
}
