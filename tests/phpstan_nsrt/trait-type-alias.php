<?php

namespace TraitTypeAlias;

use function PHPStan\Testing\assertType;

trait NoAlias
{

	/**
	 * @param Foo $p
	 */
	public function doFoo($p): void
	{
		assertType(Foo::class, $p);
	}

}

/**
 * @phpstan-type Foo array{1}
 */
class UsesNoAlias
{

	use NoAlias;

}

/**
 * @phpstan-type Foo array{2}
 */
trait WithAlias
{

	/**
	 * @param Foo $p
	 */
	public function doFoo($p): void
	{
		assertType('array{2}', $p);
	}

}

/**
 * @phpstan-type Foo array{1}
 */
class UsesWithAlias
{

	use WithAlias;

}
