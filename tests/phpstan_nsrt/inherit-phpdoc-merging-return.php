<?php declare(strict_types = 1);

namespace InheritDocMergingReturn;

use function PHPStan\Testing\assertType;

class A {}
class B extends A {}
class C extends B {}
class D extends A {}

class GrandparentClass
{
	/** @return B */
	public function method() { return new B(); }
}

interface InterfaceC
{
	/** @return C */
	public function method();
}

interface InterfaceA
{
	/** @return A */
	public function method();
}

class ParentClass extends GrandparentClass implements InterfaceC, InterfaceA
{
	/** Some comment */
	public function method() { }
}

class ChildClass extends ParentClass
{
	public function method() { }
}

class ChildClass2 extends ParentClass
{
	/**
	 * @return D
	 */
	public function method() { }
}


function (ParentClass $foo): void {
	assertType('InheritDocMergingReturn\B', $foo->method());
};

function (ChildClass $foo): void {
	assertType('InheritDocMergingReturn\B', $foo->method());
};

function (ChildClass2 $foo): void {
	assertType('InheritDocMergingReturn\D', $foo->method());
};
