<?php

namespace OverridePropertyUnionTypeWithTagSubtype;

use function PHPStan\Testing\assertType;

class HelloWorld
{
	public function __construct(
		public readonly ?int $hello,
	) {
	}
}

/** @property int $hello */
final class HelloWorld2 extends HelloWorld
{
	public function __construct(
		int $hello
	) {
		parent::__construct(hello: $hello);
	}
}

function hello2(HelloWorld2 $bla): void
{
	assertType('int', $bla->hello);  // SKIP
}
