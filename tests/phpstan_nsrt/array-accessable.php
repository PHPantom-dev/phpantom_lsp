<?php

namespace ArrayAccesable;

use function PHPStan\Testing\assertType;

class Foo implements \ArrayAccess
{

	public function __construct()
	{
		assertType('string', $this->returnArrayOfStrings()[0]);
		assertType('mixed', $this->returnMixed()[0]);
		assertType('int', $this->returnSelfWithIterableInt()[0]);
		assertType('int', $this[0]);
	}

	/**
	 * @return string[]
	 */
	public function returnArrayOfStrings(): array
	{

	}

	/**
	 * @return mixed
	 */
	public function returnMixed()
	{

	}

	/**
	 * @return self|int[]
	 */
	public function returnSelfWithIterableInt(): self
	{

	}

	#[\ReturnTypeWillChange]
	public function offsetExists($offset)
	{

	}

	public function offsetGet($offset): int
	{

	}

	#[\ReturnTypeWillChange]
	public function offsetSet($offset, $value)
	{

	}

	#[\ReturnTypeWillChange]
	public function offsetUnset($offset)
	{

	}

}
