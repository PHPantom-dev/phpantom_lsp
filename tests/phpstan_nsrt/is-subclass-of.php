<?php

namespace IsSubclassOf;

function (Bar $a, Bar $b, Bar $c, Bar $d) {
	if (is_subclass_of($a, Bar::class)) {
		\PHPStan\Testing\assertType('IsSubclassOf\Bar', $a); // Can still be a Bar child
	}

	if (is_subclass_of($b, Foo::class)) {
		\PHPStan\Testing\assertType('IsSubclassOf\Bar', $b);
	}

	/** @var class-string<Bar> $barClassString */
	$barClassString = 'Bar';
	if (is_subclass_of($c, $barClassString)) {
		\PHPStan\Testing\assertType('IsSubclassOf\Bar', $c);
	}

	/** @var class-string<Foo> $fooClassString */
	$fooClassString = 'Foo';
	if (is_subclass_of($d, $fooClassString)) {
		\PHPStan\Testing\assertType('IsSubclassOf\Bar', $d);
	}
};

function (string $a, string $b, string $c, string $d) {
	/** @var class-string<Bar> $a */
	if (is_subclass_of($a, Bar::class)) {
		\PHPStan\Testing\assertType('class-string<IsSubclassOf\Bar>', $a);
	}

	/** @var class-string<Bar> $b */
	if (is_subclass_of($b, Foo::class)) {
		\PHPStan\Testing\assertType('class-string<IsSubclassOf\Bar>', $b);
	}

	/** @var class-string<Bar> $c */
	/** @var class-string<Bar> $barClassString */
	$barClassString = 'Bar';
	if (is_subclass_of($c, $barClassString)) {
		\PHPStan\Testing\assertType('class-string<IsSubclassOf\Bar>', $c);
	}

	/** @var class-string<Bar> $d */
	/** @var class-string<Foo> $fooClassString */
	$fooClassString = 'Foo';
	if (is_subclass_of($d, $fooClassString)) {
		\PHPStan\Testing\assertType('class-string<IsSubclassOf\Bar>', $d);
	}
};

class Foo {}

class Bar extends Foo {}

final class FinalFoo {}

function (FinalFoo $a) {
	if (is_subclass_of($a, FinalFoo::class)) {
	}
};
