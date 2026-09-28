<?php

namespace IsA;

function (object $foo) {
	if (is_a($foo, Foo::class)) {
		\PHPStan\Testing\assertType('IsA\Foo', $foo);
	}
};

function (object $foo) {
	/** @var class-string<Foo> $fooClassString */
	$fooClassString = 'Foo';

	if (is_a($foo, $fooClassString)) {
		\PHPStan\Testing\assertType('IsA\Foo', $foo);
	}
};

function (string $foo) {
	if (is_a($foo, Foo::class, true)) {
		\PHPStan\Testing\assertType('class-string<IsA\Foo>', $foo);
	}
};

function (string $foo, string $someString) {
	if (is_a($foo, $someString, true)) {
		\PHPStan\Testing\assertType('class-string', $foo);
	}
};

function (Bar $a, Bar $b, Bar $c, Bar $d) {
	if (is_a($a, Bar::class)) {
		\PHPStan\Testing\assertType('IsA\Bar', $a);
	}

	if (is_a($b, Foo::class)) {
		\PHPStan\Testing\assertType('IsA\Bar', $b);
	}

	/** @var class-string<Bar> $barClassString */
	$barClassString = 'Bar';
	if (is_a($c, $barClassString)) {
		\PHPStan\Testing\assertType('IsA\Bar', $c);
	}

	/** @var class-string<Foo> $fooClassString */
	$fooClassString = 'Foo';
	if (is_a($d, $fooClassString)) {
		\PHPStan\Testing\assertType('IsA\Bar', $d);
	}
};

function (string $a, string $b, string $c, string $d) {
	/** @var class-string<Bar> $a */
	if (is_a($a, Bar::class, true)) {
		\PHPStan\Testing\assertType('class-string<IsA\Bar>', $a);
	}

	/** @var class-string<Bar> $b */
	if (is_a($b, Foo::class, true)) {
		\PHPStan\Testing\assertType('class-string<IsA\Bar>', $b);
	}

	/** @var class-string<Bar> $c */
	/** @var class-string<Bar> $barClassString */
	$barClassString = 'Bar';
	if (is_a($c, $barClassString, true)) {
		\PHPStan\Testing\assertType('class-string<IsA\Bar>', $c);
	}

	/** @var class-string<Bar> $d */
	/** @var class-string<Foo> $fooClassString */
	$fooClassString = 'Foo';
	if (is_a($d, $fooClassString, true)) {
		\PHPStan\Testing\assertType('class-string<IsA\Bar>', $d);
	}
};

class Foo {}

class Bar extends Foo {}
