<?php

namespace ConstExprPhpDocType;

use RecursiveIteratorIterator as Rec;
use function PHPStan\Testing\assertType;

class Foo
{

	public const SOME_CONSTANT = 1;
	public const SOME_OTHER_CONSTANT = 2;
	public const YET_ONE_CONST = 3;
	public const YET_ANOTHER_CONST = 4;
	public const DUMMY_SOME_CONSTANT = 99; // should only match the *

	/**
	 * @param 'foo'|'bar' $one
	 * @param self::SOME_* $two
	 * @param self::SOME_OTHER_CONSTANT $three
	 * @param \ConstExprPhpDocType\Foo::SOME_CONSTANT $four
	 * @param Rec::LEAVES_ONLY $five
	 * @param 1.0 $six
	 * @param 234 $seven
	 * @param self::SOME_OTHER_* $eight
	 * @param self::* $nine
	 * @param self::*_CONST $ten
	 * @param self::YET_*_CONST $eleven
	 * @param self::*OTHER* $twelve
	 */
	public function doFoo(
		$one,
		$two,
		$three,
		$four,
		$five,
		$six,
		$seven,
		$eight,
		$nine,
		$ten,
		$eleven,
		$twelve
	)
	{
		assertType("'bar'|'foo'", $one);
		assertType('1|2', $two);
		assertType('2', $three);
		assertType('1', $four);
		assertType('0', $five);
		assertType('1.0', $six);
		assertType('234', $seven);
		assertType('2', $eight);
		assertType('1|2|3|4|99', $nine);
		assertType('3|4', $ten);
		assertType('3|4', $eleven);
		assertType('2|4', $twelve);
	}

}
