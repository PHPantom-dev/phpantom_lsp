<?php

namespace Bug5530;

use function PHPStan\Testing\assertType;

class Foo
{
	/** @var string */
	public const TYPE_A = 'a';
	/** @var string */
	public const TYPE_B = 'b';

	/**
	 * @param self::TYPE_* $arg
	 */
	public function run(string $arg = self::TYPE_A): void
	{
		assertType('\'a\'|\'b\'', $arg);
	}

	/**
	 * @param self::TYPE_A|self::TYPE_B $arg
	 */
	public function run2(string $arg = self::TYPE_A): void
	{
		assertType('\'a\'|\'b\'', $arg);
	}
}
