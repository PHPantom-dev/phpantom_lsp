<?php // lint >= 8.3

namespace Bug10212;

use function PHPStan\Testing\assertType;

enum Foo
{
	case Bar;
}

class HelloWorld
{
	public const string A = 'foo';
	public const X\Foo B = Foo::Bar;
	public const Foo C = Foo::Bar;
}

function(HelloWorld $hw): void {
	assertType(X\Foo::class, $hw::B);
	assertType(Foo::class, $hw::C);
};
