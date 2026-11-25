<?php // lint >= 8.2

namespace Dnf;

use function PHPStan\Testing\assertType;

interface A {}
interface B {}
interface D {}

class Foo
{

	public function doFoo((A&B)|D $a): void
	{
		assertType('(Dnf\A&Dnf\B)|Dnf\D', $a);
		assertType('(Dnf\A&Dnf\B)|Dnf\D', $this->doBar());
	}

	public function doBar(): (A&B)|D
	{

	}

}
