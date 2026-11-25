<?php // lint >= 8.1

declare(strict_types=1);

namespace EnumFrom;

use function PHPStan\Testing\assertType;

enum FooIntegerEnum: int
{

	case BAR = 1;
	case BAZ = 2;

}

enum FooIntegerEnumSubset: int
{

	case BAR = 1;

}

enum FooStringEnum: string
{

	case BAR = 'bar';
	case BAZ = 'baz';

}

enum FooNumericStringEnum: string
{

	case ONE = '1';

}

class Foo
{

	public function doFoo(): void
	{
		assertType('1', FooIntegerEnum::BAR->value);
		assertType('EnumFrom\FooIntegerEnum::BAR', FooIntegerEnum::BAR);

		assertType('null', FooIntegerEnum::tryFrom(0));
		assertType(FooIntegerEnum::class, FooIntegerEnum::from(0));
		assertType('EnumFrom\FooIntegerEnum::BAR', FooIntegerEnum::tryFrom(0 + 1));
		assertType('EnumFrom\FooIntegerEnum::BAR', FooIntegerEnum::from(1 * FooIntegerEnum::BAR->value));

		assertType('EnumFrom\FooIntegerEnum::BAZ', FooIntegerEnum::tryFrom(2));
		assertType('EnumFrom\FooIntegerEnum::BAZ', FooIntegerEnum::tryFrom(FooIntegerEnum::BAZ->value));
		assertType('EnumFrom\FooIntegerEnum::BAZ', FooIntegerEnum::from(FooIntegerEnum::BAZ->value));

		assertType("'bar'", FooStringEnum::BAR->value);
		assertType('EnumFrom\FooStringEnum::BAR', FooStringEnum::BAR);

		assertType('null', FooStringEnum::tryFrom('barz'));
		assertType(FooStringEnum::class, FooStringEnum::from('barz'));

		assertType('EnumFrom\FooStringEnum::BAR', FooStringEnum::tryFrom('ba' . 'r'));
		assertType('EnumFrom\FooStringEnum::BAR', FooStringEnum::from(sprintf('%s%s', 'ba', 'r'))); // SKIP: sprintf() of literal arguments is not folded to a literal

		assertType('EnumFrom\FooStringEnum::BAZ', FooStringEnum::tryFrom('baz'));
		assertType('EnumFrom\FooStringEnum::BAZ', FooStringEnum::tryFrom(FooStringEnum::BAZ->value));
		assertType('EnumFrom\FooStringEnum::BAZ', FooStringEnum::from(FooStringEnum::BAZ->value));

		assertType('null', FooIntegerEnum::tryFrom('1')); // SKIP: a value of another type than the backing type is coerced or a TypeError depending on strict_types, so the declared type is kept
		assertType('null', FooIntegerEnum::tryFrom(1.0)); // SKIP: a value of another type than the backing type is coerced or a TypeError depending on strict_types, so the declared type is kept
		assertType('null', FooIntegerEnum::tryFrom(1.0001)); // SKIP: a value of another type than the backing type is coerced or a TypeError depending on strict_types, so the declared type is kept
		assertType('null', FooIntegerEnum::tryFrom(true)); // SKIP: a value of another type than the backing type is coerced or a TypeError depending on strict_types, so the declared type is kept
		assertType('null', FooNumericStringEnum::tryFrom(1)); // SKIP: a value of another type than the backing type is coerced or a TypeError depending on strict_types, so the declared type is kept
	}

	public function supersetToSubset(FooIntegerEnum $foo): void
	{
		assertType('EnumFrom\FooIntegerEnumSubset::BAR|null', FooIntegerEnumSubset::tryFrom($foo->value));
		assertType('EnumFrom\FooIntegerEnumSubset::BAR', FooIntegerEnumSubset::from($foo->value));
	}

	public function subsetToSuperset(FooIntegerEnumSubset $foo): void
	{
		assertType('EnumFrom\FooIntegerEnum::BAR', FooIntegerEnum::tryFrom($foo->value));
		assertType('EnumFrom\FooIntegerEnum::BAR', FooIntegerEnum::from($foo->value));
	}

	public function doCaseInsensitive(): void
	{
		assertType('1', FooInTeGerEnum::BAR->value);
		assertType('null', FooInTeGerEnum::tryFrom(0));
	}

}
