<?php

declare(strict_types=1);

namespace LooseSemantics;

use function PHPStan\Testing\assertType;

class HelloWorld
{
	/**
	 * @param true $true
	 * @param false $false
	 * @param 1 $one
	 * @param 0 $zero
	 * @param -1 $minusOne
	 * @param '1' $oneStr
	 * @param '0' $zeroStr
	 * @param '-1' $minusOneStr
	 * @param '+1' $plusOneStr
	 * @param null $null
	 * @param array{} $emptyArr
	 * @param 'php' $phpStr
	 * @param '' $emptyStr
	 */
	public function sayTrue(
		$true,
		$false,
		$one,
		$zero,
		$minusOne,
		$oneStr,
		$zeroStr,
		$minusOneStr,
		$plusOneStr,
		$null,
		$emptyArr,
		$phpStr,
		$emptyStr
	): void
	{
	}

	/**
	 * @param true $true
	 * @param false $false
	 * @param 1 $one
	 * @param 0 $zero
	 * @param -1 $minusOne
	 * @param '1' $oneStr
	 * @param '0' $zeroStr
	 * @param '-1' $minusOneStr
	 * @param '+1' $plusOneStr
	 * @param null $null
	 * @param array{} $emptyArr
	 * @param 'php' $phpStr
	 * @param '' $emptyStr
	 */
	public function sayFalse(
		$true,
		$false,
		$one,
		$zero,
		$minusOne,
		$oneStr,
		$zeroStr,
		$minusOneStr,
		$plusOneStr,
		$null,
		$emptyArr,
		$phpStr,
		$emptyStr
	): void
	{
	}

	/**
	 * @param true $true
	 * @param false $false
	 * @param 1 $one
	 * @param 0 $zero
	 * @param -1 $minusOne
	 * @param '1' $oneStr
	 * @param '0' $zeroStr
	 * @param '-1' $minusOneStr
	 * @param '+1' $plusOneStr
	 * @param null $null
	 * @param array{} $emptyArr
	 * @param 'php' $phpStr
	 * @param '' $emptyStr
	 */
	public function sayOne(
		$true,
		$false,
		$one,
		$zero,
		$minusOne,
		$oneStr,
		$zeroStr,
		$minusOneStr,
		$plusOneStr,
		$null,
		$emptyArr,
		$phpStr,
		$emptyStr
	): void
	{
		assertType('true', $one == $one);
		assertType('false', $one == $zero);
		assertType('false', $one == $minusOne);
	}

	/**
	 * @param true $true
	 * @param false $false
	 * @param 1 $one
	 * @param 0 $zero
	 * @param -1 $minusOne
	 * @param '1' $oneStr
	 * @param '0' $zeroStr
	 * @param '-1' $minusOneStr
	 * @param '+1' $plusOneStr
	 * @param null $null
	 * @param array{} $emptyArr
	 * @param 'php' $phpStr
	 * @param '' $emptyStr
	 */
	public function sayZero(
		$true,
		$false,
		$one,
		$zero,
		$minusOne,
		$oneStr,
		$zeroStr,
		$minusOneStr,
		$plusOneStr,
		$null,
		$emptyArr,
		$phpStr,
		$emptyStr
	): void
	{
		assertType('false', $zero == $one);
		assertType('true', $zero == $zero);
		assertType('false', $zero == $minusOne);
	}

	/**
	 * @param true $true
	 * @param false $false
	 * @param 1 $one
	 * @param 0 $zero
	 * @param -1 $minusOne
	 * @param '1' $oneStr
	 * @param '0' $zeroStr
	 * @param '-1' $minusOneStr
	 * @param '+1' $plusOneStr
	 * @param null $null
	 * @param array{} $emptyArr
	 * @param 'php' $phpStr
	 * @param '' $emptyStr
	 */
	public function sayMinusOne(
		$true,
		$false,
		$one,
		$zero,
		$minusOne,
		$oneStr,
		$zeroStr,
		$minusOneStr,
		$plusOneStr,
		$null,
		$emptyArr,
		$phpStr,
		$emptyStr
	): void
	{
		assertType('false', $minusOne == $one);
		assertType('false', $minusOne == $zero);
		assertType('true', $minusOne == $minusOne);
	}

	/**
	 * @param true $true
	 * @param false $false
	 * @param 1 $one
	 * @param 0 $zero
	 * @param -1 $minusOne
	 * @param '1' $oneStr
	 * @param '0' $zeroStr
	 * @param '-1' $minusOneStr
	 * @param '+1' $plusOneStr
	 * @param null $null
	 * @param array{} $emptyArr
	 * @param 'php' $phpStr
	 * @param '' $emptyStr
	 */
	public function sayOneStr(
		$true,
		$false,
		$one,
		$zero,
		$minusOne,
		$oneStr,
		$zeroStr,
		$minusOneStr,
		$plusOneStr,
		$null,
		$emptyArr,
		$phpStr,
		$emptyStr
	): void
	{
	}

	/**
	 * @param true $true
	 * @param false $false
	 * @param 1 $one
	 * @param 0 $zero
	 * @param -1 $minusOne
	 * @param '1' $oneStr
	 * @param '0' $zeroStr
	 * @param '-1' $minusOneStr
	 * @param '+1' $plusOneStr
	 * @param null $null
	 * @param array{} $emptyArr
	 * @param 'php' $phpStr
	 * @param '' $emptyStr
	 */
	public function sayZeroStr(
		$true,
		$false,
		$one,
		$zero,
		$minusOne,
		$oneStr,
		$zeroStr,
		$minusOneStr,
		$plusOneStr,
		$null,
		$emptyArr,
		$phpStr,
		$emptyStr
	): void
	{
	}

	/**
	 * @param true $true
	 * @param false $false
	 * @param 1 $one
	 * @param 0 $zero
	 * @param -1 $minusOne
	 * @param '1' $oneStr
	 * @param '0' $zeroStr
	 * @param '-1' $minusOneStr
	 * @param '+1' $plusOneStr
	 * @param null $null
	 * @param array{} $emptyArr
	 * @param 'php' $phpStr
	 * @param '' $emptyStr
	 */
	public function sayMinusOneStr(
		$true,
		$false,
		$one,
		$zero,
		$minusOne,
		$oneStr,
		$zeroStr,
		$minusOneStr,
		$plusOneStr,
		$null,
		$emptyArr,
		$phpStr,
		$emptyStr
	): void
	{
	}

	/**
	 * @param true $true
	 * @param false $false
	 * @param 1 $one
	 * @param 0 $zero
	 * @param -1 $minusOne
	 * @param '1' $oneStr
	 * @param '0' $zeroStr
	 * @param '-1' $minusOneStr
	 * @param '+1' $plusOneStr
	 * @param null $null
	 * @param array{} $emptyArr
	 * @param 'php' $phpStr
	 * @param '' $emptyStr
	 */
	public function sayPlusOneStr(
		$true,
		$false,
		$one,
		$zero,
		$minusOne,
		$oneStr,
		$zeroStr,
		$minusOneStr,
		$plusOneStr,
		$null,
		$emptyArr,
		$phpStr,
		$emptyStr
	): void
	{
	}

	/**
	 * @param true $true
	 * @param false $false
	 * @param 1 $one
	 * @param 0 $zero
	 * @param -1 $minusOne
	 * @param '1' $oneStr
	 * @param '0' $zeroStr
	 * @param '-1' $minusOneStr
	 * @param '+1' $plusOneStr
	 * @param null $null
	 * @param array{} $emptyArr
	 * @param 'php' $phpStr
	 * @param '' $emptyStr
	 */
	public function sayNull(
		$true,
		$false,
		$one,
		$zero,
		$minusOne,
		$oneStr,
		$zeroStr,
		$minusOneStr,
		$plusOneStr,
		$null,
		$emptyArr,
		$phpStr,
		$emptyStr
	): void
	{
	}

	/**
	 * @param true $true
	 * @param false $false
	 * @param 1 $one
	 * @param 0 $zero
	 * @param -1 $minusOne
	 * @param '1' $oneStr
	 * @param '0' $zeroStr
	 * @param '-1' $minusOneStr
	 * @param '+1' $plusOneStr
	 * @param null $null
	 * @param array{} $emptyArr
	 * @param 'php' $phpStr
	 * @param '' $emptyStr
	 */
	public function sayEmptyArray(
		$true,
		$false,
		$one,
		$zero,
		$minusOne,
		$oneStr,
		$zeroStr,
		$minusOneStr,
		$plusOneStr,
		$null,
		$emptyArr,
		$phpStr,
		$emptyStr
	): void
	{
	}

	/**
	 * @param true $true
	 * @param false $false
	 * @param 1 $one
	 * @param 0 $zero
	 * @param -1 $minusOne
	 * @param '1' $oneStr
	 * @param '0' $zeroStr
	 * @param '-1' $minusOneStr
	 * @param '+1' $plusOneStr
	 * @param null $null
	 * @param array{} $emptyArr
	 * @param 'php' $phpStr
	 * @param '' $emptyStr
	 * @param non-falsy-string $nonFalsyString
	 */
	public function sayNonFalsyStr(
		$true,
		$false,
		$one,
		$zero,
		$minusOne,
		$oneStr,
		$zeroStr,
		$minusOneStr,
		$plusOneStr,
		$null,
		$emptyArr,
		$phpStr,
		$emptyStr,
		$nonFalsyString
	): void
	{

		assertType('bool', $nonFalsyString == $true);
		assertType('bool', $nonFalsyString == $one);
		assertType('bool', $nonFalsyString == $minusOne);
		assertType('bool', $nonFalsyString == $oneStr);
		assertType('bool', $nonFalsyString == $minusOneStr);
		assertType('bool', $nonFalsyString == $plusOneStr);
		assertType('bool', $nonFalsyString == $phpStr);
	}

	/**
	 * @param true $true
	 * @param false $false
	 * @param 1 $one
	 * @param 0 $zero
	 * @param -1 $minusOne
	 * @param '1' $oneStr
	 * @param '0' $zeroStr
	 * @param '-1' $minusOneStr
	 * @param '+1' $plusOneStr
	 * @param null $null
	 * @param array{} $emptyArr
	 * @param 'php' $phpStr
	 * @param '' $emptyStr
	 * @param numeric-string $numericStr
	 */
	public function sayStr(
		$true,
		$false,
		$one,
		$zero,
		$minusOne,
		$oneStr,
		$zeroStr,
		$minusOneStr,
		$plusOneStr,
		$null,
		$emptyArr,
		string $string,
		$phpStr,
		$emptyStr,
		$numericStr,
		?string $stringOrNull,
	): void
	{
		assertType('bool', $string == $true);
		assertType('bool', $string == $false);
		assertType('bool', $string == $one);
		assertType('bool', $string == $zero);
		assertType('bool', $string == $minusOne);
		assertType('bool', $string == $oneStr);
		assertType('bool', $string == $zeroStr);
		assertType('bool', $string == $minusOneStr);
		assertType('bool', $string == $plusOneStr);
		assertType('bool', $string == $null);
		assertType('bool', $string == $stringOrNull);
		assertType('bool', $string == $phpStr);
		assertType('bool', $string == $emptyStr);
		assertType('bool', $string == $numericStr);

		assertType('bool', $numericStr == $true);
		assertType('bool', $numericStr == $false);
		assertType('bool', $numericStr == $one);
		assertType('bool', $numericStr == $zero);
		assertType('bool', $numericStr == $minusOne);
		assertType('bool', $numericStr == $oneStr);
		assertType('bool', $numericStr == $zeroStr);
		assertType('bool', $numericStr == $minusOneStr);
		assertType('bool', $numericStr == $plusOneStr);
		assertType('bool', $numericStr == $stringOrNull);
		assertType('bool', $numericStr == $string);
		if (is_numeric($string)) {
			assertType('bool', $numericStr == $string);
		}

	}

	/**
	 * @param true $true
	 * @param false $false
	 * @param 1 $one
	 * @param 0 $zero
	 * @param -1 $minusOne
	 * @param '1' $oneStr
	 * @param '0' $zeroStr
	 * @param '-1' $minusOneStr
	 * @param '+1' $plusOneStr
	 * @param null $null
	 * @param array{} $emptyArr
	 * @param 'php' $phpStr
	 * @param '' $emptyStr
	 */
	public function sayEmptyStr(
		$true,
		$false,
		$one,
		$zero,
		$minusOne,
		$oneStr,
		$zeroStr,
		$minusOneStr,
		$plusOneStr,
		$null,
		$emptyArr,
		$phpStr,
		$emptyStr
	): void
	{
	}

	/**
	 * @param true $true
	 * @param false $false
	 * @param 1 $one
	 * @param 0 $zero
	 * @param -1 $minusOne
	 * @param '1' $oneStr
	 * @param '0' $zeroStr
	 * @param '-1' $minusOneStr
	 * @param '+1' $plusOneStr
	 * @param null $null
	 * @param array{} $emptyArr
	 * @param 'php' $phpStr
	 * @param '' $emptyStr
	 * @param int<10, 20> $positiveIntRange
	 * @param int<-20, -10> $negativeIntRange
	 * @param int<-10, 10> $minusTenToTen
	 */
	public function sayInt(
		$true,
		$false,
		$one,
		$zero,
		$minusOne,
		$oneStr,
		$zeroStr,
		$minusOneStr,
		$plusOneStr,
		$null,
		$emptyArr,
		array $array,
		int $int,
		int $intRange,
		string $emptyStr,
		string $phpStr,
		int $positiveIntRange,
		int $negativeIntRange,
		int $minusTenToTen,
	): void
	{
		assertType('bool', $int == $true);
		assertType('bool', $int == $false);
		assertType('bool', $int == $one);
		assertType('bool', $int == $zero);
		assertType('bool', $int == $minusOne);
		assertType('bool', $int == $oneStr);
		assertType('bool', $int == $zeroStr);
		assertType('bool', $int == $minusOneStr);
		assertType('bool', $int == $plusOneStr);
		assertType('bool', $int == $null);



		// see https://3v4l.org/VudDK
		assertType('bool', $minusTenToTen == $true);
		assertType('bool', $minusTenToTen == $false);
		assertType('bool', $minusTenToTen == $one);
		assertType('bool', $minusTenToTen == $zero);
		assertType('bool', $minusTenToTen == $minusOne);
		assertType('bool', $minusTenToTen == $oneStr);
		assertType('bool', $minusTenToTen == $zeroStr);
		assertType('bool', $minusTenToTen == $minusOneStr);
		assertType('bool', $minusTenToTen == $plusOneStr);
		assertType('bool', $minusTenToTen == $null);

		// see https://3v4l.org/oJl3K
		assertType('bool', $minusTenToTen > $null);
		assertType('bool', $minusTenToTen <= $null);

		// see https://3v4l.org/oRSgU
		assertType('bool', $null < $minusTenToTen);
		assertType('bool', $null >= $minusTenToTen);



	}

	/**
	 * @param true|1|"1" $looseOne
	 * @param false|0|"0" $looseZero
	 * @param false|1 $constMix
	 * @param "abc"|"def" $constNonFalsy
	 * @param array{abc: string, num?: int, nullable: ?string} $arrShape
	 * @param array{} $emptyArr
	 */
	public function sayConstUnion(
		$looseOne,
		$looseZero,
		$constMix,
		$constNonFalsy,
		array $arrShape,
		array $emptyArr
	): void
	{

		assertType('bool', $looseZero == []);

		assertType('bool', $constMix == 0);
		assertType('bool', $constMix == 1);
		assertType('bool', $constMix == true);
		assertType('bool', $constMix == false);
		assertType('bool', $constMix == "1");
		assertType('bool', $constMix == "0");
		assertType('bool', $constMix == []);

		assertType('bool', $looseOne == $constMix);
		assertType('bool', $constMix == $looseOne);
		assertType('bool', $looseZero == $constMix);
		assertType('bool', $constMix == $looseZero);


		assertType('bool', $emptyArr == $constMix);
		assertType('bool', $emptyArr == $looseZero);

		assertType('bool', $arrShape == $looseOne);
		assertType('bool', $arrShape == $constMix);
		assertType('bool', $arrShape == $looseZero);
	}

	/**
	 * @param uppercase-string $upper
	 * @param lowercase-string $lower
	 * @param array{} $emptyArr
	 * @param non-empty-array $nonEmptyArr
	 * @param array{abc: string, num?: int, nullable: ?string} $arrShape
	 * @param int<10, 20> $intRange
	 */
	public function sayIntersection(
		string $upper,
		string $lower,
		string $s,
		array $emptyArr,
		array $nonEmptyArr,
		array $arr,
		array $arrShape,
		int $i,
		int $intRange,
	): void
	{
		// https://3v4l.org/q8OP2
		assertType('false', '1e2' === '1E2');

		assertType('bool', '' == $upper);
		assertType('bool', '0' == $upper);
		assertType('bool', '1e2' == $upper);
		assertType('bool', strtoupper($s) == $upper);
		assertType('bool', strtolower($s) == $upper);
		assertType('bool', $upper == $lower);

		assertType('bool', '0' == $lower);
		assertType('bool', '1E2' == $lower);
		assertType('bool', strtoupper($s) == $lower);
		assertType('bool', strtolower($s) == $lower);
		assertType('bool', $lower == $upper);

		assertType('bool', $arr == $nonEmptyArr);
		assertType('bool', $nonEmptyArr == $arr);

		if (count($arr) > 0) {
		}

		assertType('bool', '' == $lower);
		if ($lower != '') {
		}
		if ($upper != '') {
		}
	}

}
