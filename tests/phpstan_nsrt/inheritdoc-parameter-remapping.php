<?php

namespace InheritDocParameterRemapping;

use function PHPStan\Testing\assertType;

class Lorem
{

	/**
	 * @param B $b
	 * @param C $c
	 * @param A $a
	 * @param D $d
	 */
	public function doLorem($a, $b, $c, $d)
	{

	}

}

class Ipsum extends Lorem
{

	public function doLorem($x, $y, $z, $d)
	{
		assertType('InheritDocParameterRemapping\A', $x);
		assertType('InheritDocParameterRemapping\B', $y);
		assertType('InheritDocParameterRemapping\C', $z);
		assertType('InheritDocParameterRemapping\D', $d);
	}

}

class Dolor extends Ipsum
{

	public function doLorem($g, $h, $i, $d)
	{
		assertType('InheritDocParameterRemapping\A', $g);
		assertType('InheritDocParameterRemapping\B', $h);
		assertType('InheritDocParameterRemapping\C', $i);
		assertType('InheritDocParameterRemapping\D', $d);
	}

}
