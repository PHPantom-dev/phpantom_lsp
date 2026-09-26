<?php // lint >= 8.3

namespace ReturnTypeClassConstant;

use function PHPStan\Testing\assertType;

enum Foo
{

	const static FOO = Foo::A;

	case A;

	public function returnStatic(): static
	{
		assertType('ReturnTypeClassConstant\Foo::A', self::FOO); // SKIP: a class constant whose initializer names an enum case is not resolved
		return self::FOO;
	}

	public function returnStatic2(self $self): static
	{
		assertType('ReturnTypeClassConstant\Foo::A', $self::FOO); // SKIP: a class constant read through an object ($obj::CONST) resolves to nothing
		return $self::FOO;
	}

}

function (Foo $foo): void {
	assertType('ReturnTypeClassConstant\Foo::A', Foo::FOO); // SKIP: a class constant whose initializer names an enum case is not resolved
	assertType('ReturnTypeClassConstant\Foo::A', $foo::FOO); // SKIP: a class constant read through an object ($obj::CONST) resolves to nothing
};
