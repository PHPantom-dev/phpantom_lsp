<?php

namespace MixinTraitUse;

use function PHPStan\Testing\assertType;

/**
 * @template T
 */
interface Foo
{

	/** @return T */
	public function get();

}

/**
 * @mixin Foo<static>
 * @mixin string&int
 */
trait FooTrait
{

}

class Usages
{

	use FooTrait;

}

function (Usages $u): void {
	assertType(Usages::class, $u->get());
};
