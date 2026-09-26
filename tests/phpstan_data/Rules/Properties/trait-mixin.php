<?php

namespace TraitMixinProperties;

use function PHPStan\Testing\assertType;

/**
 * @template T
 */
class Foo
{

	/** @var T */
	public $a;

}

/**
 * @mixin Foo<static>
 */
trait FooTrait
{

}

#[\AllowDynamicProperties]
class Usages
{

	use FooTrait;

}

class ChildUsages extends Usages
{

}

function (Usages $u): void {
	assertType(Usages::class, $u->a); // SKIP: @mixin on a trait is not applied to the class that uses it
};

function (ChildUsages $u): void {
	assertType(ChildUsages::class, $u->a); // SKIP: @mixin on a trait is not applied to the class that uses it
};
