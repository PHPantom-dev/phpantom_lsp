<?php declare(strict_types = 1);

namespace Analyser\Bug3142;

use function PHPStan\Testing\assertType;

class ParentClass
{

	/**
	 * @return int
	 */
	public function sayHi()
	{
		return 'hi';
	}

}

/**
 * @method string sayHi()
 * @method string sayHello()
 */
class HelloWorld extends ParentClass
{
	/**
	 * @return int
	 */
	public function sayHello()
	{
		return 'hello';
	}
}

function (): void {
	$hw = new HelloWorld();
	assertType('string', $hw->sayHi());  // SKIP
	assertType('string', $hw->sayHello());  // SKIP
};

interface DecoratorInterface
{
}

class FooDecorator implements DecoratorInterface
{
	public function getCode(): string
	{
		return 'FOO';
	}
}

trait DecoratorTrait
{
	public function getDecorator(): DecoratorInterface
	{
		return new FooDecorator();
	}
}

/**
 * @method FooDecorator getDecorator()
 */
class Dummy
{
	use DecoratorTrait;

	public function getLabel(): string
	{
		return $this->getDecorator()->getCode();
	}
}

function () {
	$dummy = new Dummy();
	assertType(FooDecorator::class, $dummy->getDecorator());  // SKIP
};
