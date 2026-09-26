<?php

namespace TraitMixinMethods;

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
 */
trait FooTrait
{

}

class Usages
{

	use FooTrait;

}

class ChildUsages extends Usages
{

}

function (Usages $u): void {
	assertType(Usages::class, $u->get()); // SKIP: @mixin on a trait is not applied to the class that uses it
};

function (ChildUsages $u): void {
	assertType(ChildUsages::class, $u->get()); // SKIP: @mixin on a trait is not applied to the class that uses it
};
