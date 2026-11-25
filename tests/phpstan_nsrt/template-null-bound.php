<?php

namespace TemplateNullBound;

use function PHPStan\Testing\assertType;

class Foo
{

	/**
	 * @template T of ?int
	 * @param T $p
	 * @return T
	 */
	public function doFoo(?int $p): ?int
	{
		return $p;
	}

}

function (Foo $f, ?int $i): void {
	assertType('null', $f->doFoo(null));
	assertType('1', $f->doFoo(1));
	assertType('int|null', $f->doFoo($i));
};
