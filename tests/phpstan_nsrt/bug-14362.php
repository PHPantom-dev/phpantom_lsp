<?php // lint >= 8.1

declare(strict_types = 1);

namespace Bug14362;

use function PHPStan\Testing\assertType;

interface A
{
	public function __invoke(B $b): int;
}

interface B
{

}

class C {
	public static function u(): A&B {
		return new class() implements A, B {
			public function __invoke(B $b): int {
				return 1;
			}
		};
	}
}

class D {
	public static function u(): A {
		return new class() implements A {
			public function __invoke(B $b): int {
				return 1;
			}
		};
	}
}

interface E
{
	public function __invoke(B $b, bool $option = true): int;
}

interface F
{

}

class G {
	public static function u(): A&E {
		return new class() implements A, E {
			public function __invoke(B $b, bool $option = true): int {
				return 1;
			}
		};
	}
}

class H {
	public static function u(): B&F {
		return new class() implements B, F {
		};
	}
}

function doBar() : void {
	assertType('Closure(Bug14362\B): int', C::u()(...));
	assertType('Closure(Bug14362\B): int', D::u()(...));

	// Intersection with two yes-callable compatible
	assertType('Closure(Bug14362\B, bool=): int', G::u()(...));

	// Intersection with only maybe-callable types (neither has __invoke)
	assertType('Closure', H::u()(...));
}

function doFoo(string $c):void {
	if (is_callable($c)) {
		$a = $c;
	} else {
		$a = C::u()(...);
	}
	assertType('callable-string|(Closure(Bug14362\B): int)', $a);
}
