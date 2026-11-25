<?php // lint >= 8.1

namespace NativeTypesFirstClassCallables;

use function PHPStan\Testing\assertNativeType;
use function PHPStan\Testing\assertType;

class Foo
{

	/** @return non-empty-string */
	public function doFoo(): string
	{

	}

	/** @return non-empty-string */
	public static function doBar(): string
	{

	}

}

/** @return non-empty-string */
function doFooFunction(): string
{

}

class Test
{

	public function doFoo(): void
	{
		$foo = new Foo();
		$f = $foo->doFoo(...);
		assertType('non-empty-string', $f());
		assertNativeType('string', $f());
		assertType('non-empty-string', ($foo->doFoo(...))());
		assertNativeType('string', ($foo->doFoo(...))());

		$g = Foo::doBar(...);
		assertType('non-empty-string', $g());
		assertNativeType('string', $g());

		$h = doFooFunction(...);
		assertType('non-empty-string', $h());
		assertNativeType('string', $h());

		$i = $h(...);
		assertType('non-empty-string', $i());
		assertNativeType('string', $i());

		$j = [Foo::class, 'doBar'](...);
		assertType('non-empty-string', $j());
		assertNativeType('string', $j());
	}

}

class Nullsafe
{

	/** @var int */
	private $untyped;

	private int $typed;

	/** @return non-empty-string */
	public function doFoo(): string
	{

	}

	public function doBar(?self $self): void
	{
		assertType('non-empty-string|null', $self?->doFoo());
		assertNativeType('string|null', $self?->doFoo());

		assertType('int|null', $self?->untyped);
		assertNativeType('mixed', $self?->untyped);
		assertType('int|null', $self?->typed);
		assertNativeType('int|null', $self?->typed);
	}

}
