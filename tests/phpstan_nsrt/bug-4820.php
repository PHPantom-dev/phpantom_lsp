<?php

namespace Bug4820;

use PHPStan\TrinaryLogic;
use function PHPStan\Testing\assertType;
use function PHPStan\Testing\assertVariableCertainty;

class Param {

	/** @var bool */
	public $foo;
}

class HelloWorld
{
	public function sayHello(Param $param): void
	{

		try {
			$result = call_user_func([$this, 'mayThrow']);
			if ($param->foo) {
				$this->mayThrow();
			}

		} catch (\Throwable $e) {
			assertType('bool', $param->foo);
			assertVariableCertainty(TrinaryLogic::createMaybe(), $result);
			throw $e;
		}
	}

	/**
	 * @throws \RuntimeException
	 */
	private function mayThrow(): void
	{
		throw new \RuntimeException();
	}

}
