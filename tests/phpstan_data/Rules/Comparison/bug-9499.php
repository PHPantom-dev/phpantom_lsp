<?php // lint >= 8.1

namespace Bug9499;

use function PHPStan\Testing\assertType;

enum FooEnum
{
	case A;
	case B;
	case C;
	case D;
}

class Foo
{
	public function __construct(public readonly FooEnum $f)
	{
	}
}

function test(FooEnum $f, Foo $foo): void
{
	$arr = ['f' => $f];
	match ($arr['f']) {
		FooEnum::A, FooEnum::B => match ($arr['f']) {
			FooEnum::A => 'a',
			FooEnum::B => 'b',
		},
		default => '',
	};
	match ($foo->f) {
		FooEnum::A, FooEnum::B => match ($foo->f) {
			FooEnum::A => 'a',
			FooEnum::B => 'b',
		},
		default => '',
	};
}

function test2(FooEnum $f, Foo $foo): void
{
	$arr = ['f' => $f];
	match ($arr['f']) {
		FooEnum::A, FooEnum::B => assertType(FooEnum::class . '::A|' . FooEnum::class . '::B', $arr['f']),
		default => '',
	};
	match ($foo->f) {
		FooEnum::A, FooEnum::B => assertType(FooEnum::class . '::A|' . FooEnum::class . '::B', $foo->f),
		default => '',
	};
}
