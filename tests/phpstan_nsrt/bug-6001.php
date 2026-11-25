<?php declare(strict_types=1);

namespace Bug6001;

use function PHPStan\Testing\assertType;

class Foo
{

	public function doFoo(\Throwable $t): void
	{
		assertType('(int|string)', (new \Exception())->getCode());  // SKIP
		assertType('(int|string)', $t->getCode());  // SKIP
		assertType('(int|string)', (new \RuntimeException())->getCode());  // SKIP
		assertType('int', (new \LogicException())->getCode());  // SKIP
		assertType('(int|string)', (new \PDOException())->getCode());  // SKIP
		assertType('int', (new MyException())->getCode());  // SKIP
		assertType('(int|string)', (new SubPDOException())->getCode());  // SKIP
		assertType('1|2|3', (new ExceptionWithMethodTag())->getCode());  // SKIP
	}

	/**
	 * @param \PDOException|MyException $exception
	 * @return void
	 */
	public function doBar($exception): void
	{
		assertType('(int|string)', $exception->getCode());  // SKIP
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
