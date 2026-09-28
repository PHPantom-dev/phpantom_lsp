<?php

namespace Bug2733;

use function PHPStan\Testing\assertType;

class Foo
{

	/**
	 * @param array{id:int, name:string} $data
	 */
	public function doSomething(array $data): void
	{
		foreach (['id', 'name'] as $required) {
			if (!isset($data[$required])) {
				throw new \Exception(sprintf('Missing data element: %s', $required));
			}
		}

		assertType('array{id: int, name: string}', $data);
	}

}
