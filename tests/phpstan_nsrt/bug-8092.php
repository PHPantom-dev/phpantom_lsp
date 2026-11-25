<?php

namespace Bug8092;

use function PHPStan\Testing\assertType;

interface Generic
{}

class Specific implements Generic
{}

/** @template-covariant T of Generic */
interface TypeWithGeneric
{
	/** @return T */
	public function get(): Generic;
}

/** @implements TypeWithGeneric<Specific> */
class TypeWithSpecific implements TypeWithGeneric
{
	public function get(): Specific
	{
		return new Specific();
	}
}

class HelloWorld
{
	/** @param TypeWithGeneric<Generic> $type */
	public function test(TypeWithGeneric $type): void
	{
		match (get_class($type)) {
			TypeWithSpecific::class => assertType(TypeWithSpecific::class, $type),
			default => false,
		};
	}
}
