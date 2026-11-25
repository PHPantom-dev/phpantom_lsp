<?php // lint >= 8.0

namespace Bug13552;

use function PHPStan\Testing\assertType;

interface MyInterface {
	public function doThing(): bool;

	/**
	 * @return array<string, string>
	 */
	public function getArray(): array;
}

function test_addition(MyInterface $i): void {
	$x = $i->doThing() ? ['thing' => 'do'] : [];
	assertType("array{}|array{thing: 'do'}", $x);

	$x += $i->getArray();
	assertType('array<string, string>', $x);

	$x = $x ?: ['test' => 'string'];
}

function more_test(MyInterface $i): void {
	$x = $i->doThing() ? ['thing' => 'do', 'always_here' => true] : ['always_here' => 42];
	assertType("array{always_here: 42}|array{thing: 'do', always_here: true}", $x);

	$a = $i->getArray() + $x;

	$b = $x + $i->getArray();
}

/**
 * @param array{thing?: 'do', always_here: 42|true} $x
 */
function more_test_2(MyInterface $i, array $x): void {
	$a = $i->getArray() + $x;

	$b = $x + $i->getArray();
}
