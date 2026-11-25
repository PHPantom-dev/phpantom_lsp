<?php

namespace Bug1233;

use function PHPStan\Testing\assertType;

class HelloWorld
{
	public function toArray($value): array
	{
		assertType('mixed', $value);
		if (is_array($value)) {
			assertType('array<mixed, mixed>', $value);
			return $value;
		}


		if (is_iterable($value)) {
			return iterator_to_array($value);
		}


		throw new \LogicException();
	}
}
