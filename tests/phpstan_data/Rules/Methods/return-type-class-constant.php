<?php // lint >= 8.3

namespace ReturnTypeClassConstant;

use function PHPStan\Testing\assertType;

enum Foo
{

	const static FOO = Foo::A;

	case A;

	public function returnStatic(): static
	{
		assertType('ReturnTypeClassConstant\Foo::A', self::FOO);
		return self::FOO;
	}

	public function returnStatic2(self $self): static
	{
		assertType('ReturnTypeClassConstant\Foo::A', $self::FOO);
		return $self::FOO;
	}

}

function (Foo $foo): void {
	assertType('ReturnTypeClassConstant\Foo::A', Foo::FOO);
	assertType('ReturnTypeClassConstant\Foo::A', $foo::FOO);
};
