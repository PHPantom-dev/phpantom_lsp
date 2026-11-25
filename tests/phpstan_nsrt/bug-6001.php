<?php declare(strict_types=1);

namespace Bug6001;

use function PHPStan\Testing\assertType;

class Foo
{

	public function doFoo(\Throwable $t): void
	{
		assertType('(int|string)', (new \Exception())->getCode());
		assertType('(int|string)', $t->getCode());
		assertType('(int|string)', (new \RuntimeException())->getCode());
		assertType('int', (new \LogicException())->getCode());
		assertType('(int|string)', (new \PDOException())->getCode());
		assertType('int', (new MyException())->getCode());
		assertType('(int|string)', (new SubPDOException())->getCode());
		assertType('1|2|3', (new ExceptionWithMethodTag())->getCode()); // SKIP: an @method tag does not override an inherited native method
	}

	/**
	 * @param \PDOException|MyException $exception
	 * @return void
	 */
	public function doBar($exception): void
	{
		assertType('(int|string)', $exception->getCode());
	}

}

class MyException extends \Exception
{

}

class SubPDOException extends \PDOException
{

}

/**
 * @method 1|2|3 getCode()
 */
class ExceptionWithMethodTag extends \Exception
{

}
