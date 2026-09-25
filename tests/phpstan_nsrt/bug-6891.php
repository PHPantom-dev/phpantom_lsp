<?php declare(strict_types = 1);

namespace Bug6891;

use function PHPStan\Testing\assertType;

class HelloWorld
{

	/**
	 * @param mixed $context
	 */
	public function sayHello($context): bool
	{
		if (is_subclass_of($context, HelloWorld::class)) {
			return true;
		}
		assertType('mixed', $context);

		return HelloWorld::class === $context;
	}

	public function sayHello2(object $context): bool
	{
		if (is_subclass_of($context, HelloWorld::class)) {
			return true;
		}
		assertType('object', $context);

		return HelloWorld::class === get_class($context);
	}

	public function sayHello3(string $context): bool
	{
		if (is_subclass_of($context, HelloWorld::class)) {
			return true;
		}
		assertType('string', $context);

		return HelloWorld::class === $context;
	}
}
