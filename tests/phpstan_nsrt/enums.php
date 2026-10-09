<?php

namespace EnumTypeAssertions;

use function PHPStan\Testing\assertType;

enum Foo
{

	case ONE;
	case TWO;

}

enum Bar : string
{

	case ONE = 'one';
	case TWO = 'two';

}

enum Baz : int
{

	case ONE = 1;
	case TWO = 2;
	const THREE = 3;
	const FOUR = 4;

}

class FooClass
{

	public function doFoo(Foo $foo): void
	{
		assertType('Foo', Foo::ONE);
		assertType('Foo', Foo::TWO);
	}

}

class BarClass
{

	public function doFoo(string $s, Bar $bar): void
	{
		assertType('Bar', Bar::ONE);
		assertType('Bar', Bar::TWO);
		assertType('\'two\'', Bar::TWO->value);

		assertType('EnumTypeAssertions\Bar', Bar::from($s));
		assertType('?EnumTypeAssertions\Bar', Bar::tryFrom($s));

		assertType('\'one\'|\'two\'', $bar->value);
	}

}

class BazClass
{

	public function doFoo(int $i, Baz $baz): void
	{
		assertType('Baz', Baz::ONE);
		assertType('Baz', Baz::TWO);
		assertType('2', Baz::TWO->value);

		assertType('EnumTypeAssertions\Baz', Baz::from($i));
		assertType('?EnumTypeAssertions\Baz', Baz::tryFrom($i));

		assertType('1|2', $baz->value);
		assertType('1', Baz::ONE->value);
		assertType('2', Baz::TWO->value);
	}

}

/** @template T */
interface GenericInterface
{

	/** @return T */
	public function doFoo();

}

/** @implements GenericInterface<int> */
enum EnumImplementsGeneric: int implements GenericInterface
{

	case ONE = 1;

	public function doFoo()
	{
		return 1;
	}

}

class TestEnumImplementsGeneric
{

	public function doFoo(EnumImplementsGeneric $e): void
	{
		assertType('int', $e->doFoo());
		assertType('int', EnumImplementsGeneric::ONE->doFoo());
	}

}

class MixedMethod
{

	public function doFoo(): int
	{
		return 1;
	}

}

/** @mixin MixedMethod */
enum EnumWithMixin
{

}

function (EnumWithMixin $i): void {
	assertType('int', $i->doFoo());
};

class Lorem
{

	public function doFoo(Foo $foo): void
	{
		if ($foo === Foo::ONE) {
			assertType('Foo', $foo);
			return;
		}

		assertType('Foo', $foo);
	}

}