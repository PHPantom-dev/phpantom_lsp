<?php declare(strict_types = 1);

namespace InheritDocMergingParam;

use function PHPStan\Testing\assertType;

class A {}
class B extends A {}
class C extends B {}

class GrandparentClass
{
	/** @param A $one */
	public function method($one, $two): void {}
}

class ParentClass extends GrandparentClass
{
	/** @param B $dos */
	public function method($uno, $dos): void
	{
		assertType('InheritDocMergingParam\A', $uno);
		assertType('InheritDocMergingParam\B', $dos);
	}
}

class ChildClass extends ParentClass
{
	/** @param C $one */
	public function method($one, $two): void
	{
		assertType('InheritDocMergingParam\C', $one);
		assertType('InheritDocMergingParam\B', $two);
	}

}
