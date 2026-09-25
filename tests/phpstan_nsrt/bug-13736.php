<?php declare(strict_types = 1);

namespace Bug13736;

use function PHPStan\Testing\assertType;

class BaseClass {
	final public static function assertTrue(mixed $condition, string $message = ''): void
	{

	}

	public function doSomething(): void
	{

	}
}

class Bug13736Test extends BaseClass
{
	private static ?Foo $foo = null;
	private ?Foo $instanceFoo = null;

	public function testStaticProperty(): void
	{
		self::$foo = new Foo();
		assertType('Bug13736\Foo', self::$foo);
		self::assertTrue(true);
		assertType('Bug13736\Foo', self::$foo);
	}

	public function testInstanceProperty(): void
	{
		$this->instanceFoo = new Foo();
		assertType('Bug13736\Foo', $this->instanceFoo);
		parent::doSomething();
		assertType('Bug13736\Foo', $this->instanceFoo);
	}
}

class Foo {
}
