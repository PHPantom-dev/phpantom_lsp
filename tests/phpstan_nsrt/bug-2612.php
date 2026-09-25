<?php

namespace Bug2612;

use function PHPStan\Testing\assertType;

class TypeFactory
{

	/**
	 * @phpstan-template T
	 * @phpstan-param T $type
	 * @phpstan-return T
	 */
	public static function singleton($type)
	{
		return $type;
	}

}

class StringType
{

	public static function create(string $value): self
	{
		$valueType = new static();
		$result = TypeFactory::singleton($valueType);
		assertType('static(Bug2612\StringType)', $result);

		return $result;
	}


}
