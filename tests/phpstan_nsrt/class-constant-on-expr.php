<?php

namespace ClassConstantOnExprAssertType;

use function PHPStan\Testing\assertType;

class Foo
{

	public function doFoo(
		\stdClass $std,
		string $string,
		?\stdClass $stdOrNull,
		?string $stringOrNull
	): void
	{
		assertType('*ERROR*', $string::class);
		assertType('class-string<stdClass>', $stdOrNull::class); // PHPStan says `(class-string<stdClass>&literal-string)|null`, but `::class` on `null` throws
		assertType('*ERROR*', $stringOrNull::class);
		assertType("'Foo'", 'Foo'::class); // SKIP: `::class` on a string literal has no type
	}

}
