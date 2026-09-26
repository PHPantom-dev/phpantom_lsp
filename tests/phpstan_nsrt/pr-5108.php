<?php declare(strict_types = 1);

namespace Pr5108;

use function PHPStan\Testing\assertType;

class A
{
}

interface I
{
	function method(): void;
}

class Testing
{
	/**
	 * @param A|null $arg
	 * @param-out ($arg is null ? A&I : A) $arg
	 */
	public static function testMethod(?A &$arg = null): void
	{
		if ($arg === null) {
			$arg = new A();
		}
	}

	public function doTest(A $a): void
	{
		Testing::testMethod($a);
		assertType('Pr5108\A', $a);

		$b = null;
		Testing::testMethod($b);
		assertType('Pr5108\A&Pr5108\I', $b);

		$d = $a->getFoo();
		assertType('*ERROR*', $d);
		Testing::testMethod($d);
		assertType('Pr5108\A', $d);
	}
}
