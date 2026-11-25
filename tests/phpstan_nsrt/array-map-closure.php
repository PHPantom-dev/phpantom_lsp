<?php

namespace ArrayMapClosure;

use function PHPStan\Testing\assertType;

class A
{

}

class B extends A
{

}

class C extends A
{

}

function (): void {
	array_map(function ($item) {
		assertType(B::class . '|' . C::class, $item);
	}, [new B(), new C()]);

	array_map(function (A $item) {
		assertType(B::class . '|' . C::class, $item);
	}, [new B(), new C()]);
};

function (): void {
	array_filter([new B(), new C()], function ($item) {
		assertType(B::class . '|' . C::class, $item);
	});

	array_filter([new B(), new C()], function (A $item) {
		assertType(B::class . '|' . C::class, $item);
	});
};
