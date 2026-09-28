<?php // lint >= 8.3

namespace ReturnTypeClassConstant;

use function PHPStan\Testing\assertType;

enum Foo
{

	const static FOO = Foo::A;

	case A;

	public function returnStatic(): static
	{
		assertType('ReturnTypeClassConstant\Foo::A', self::FOO); // SKIP: there is no type for a single enum case
		return self::FOO;
	}

	public function returnStatic2(self $self): static
	{
		assertType('ReturnTypeClassConstant\Foo::A', $self::FOO); // SKIP: there is no type for a single enum case
		return $self::FOO;
	}

}

function (Foo $foo): void {
	assertType('ReturnTypeClassConstant\Foo::A', Foo::FOO); // SKIP: there is no type for a single enum case
	assertType('ReturnTypeClassConstant\Foo::A', $foo::FOO); // SKIP: there is no type for a single enum case
};
