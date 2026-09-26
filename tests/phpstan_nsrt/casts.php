<?php

namespace TypesNamespaceCasts;

use function PHPStan\Testing\assertType;

class Bar
{

	/** @var self */
	private $barProperty;

}

class Foo extends Bar
{

	/** @var self */
	private $foo;

	/** @var int */
	private $int;

	/** @var int */
	protected $protectedInt;

	/** @var int */
	public $publicInt;

	/**
	 * @param string $str
	 */
	public function doFoo(string $str)
	{
		$castedInteger = (int) foo();
		$castedBoolean = (bool) foo();
		$castedFloat = (float) foo();
		$castedString = (string) foo();
		$castedArray = (array) foo();
		$castedObject = (object) foo();
		assertType('int', $castedInteger);
		assertType('bool', $castedBoolean);
		assertType('float', $castedFloat);
		assertType('string', $castedString);
		assertType('array', $castedArray);
		assertType('stdClass', $castedObject);

		$intFromStr = (int) $str;
		$floatFromStr = (float) $str;
		assertType('int', $intFromStr);
		assertType('float', $floatFromStr);
	}

	/**
	 * @param int $intParam
	 * @param float $floatParam
	 * @param bool $boolParam
	 * @param string $strParam
	 */
	public function castPreservesScalarTypes(
		int $intParam,
		float $floatParam,
		bool $boolParam,
		string $strParam
	): void
	{
		$castInt = (int) $intParam;
		$castFloat = (float) $floatParam;
		$castBool = (bool) $boolParam;
		$castString = (string) $strParam;

		assertType('int', $castInt);
		assertType('float', $castFloat);
		assertType('bool', $castBool);
		assertType('string', $castString);

		$intToFloat = (float) $intParam;
		$intToString = (string) $intParam;
		$intToBool = (bool) $intParam;
		assertType('float', $intToFloat);
		assertType('string', $intToString);
		assertType('bool', $intToBool);

		$floatToInt = (int) $floatParam;
		$floatToString = (string) $floatParam;
		$floatToBool = (bool) $floatParam;
		assertType('int', $floatToInt);
		assertType('string', $floatToString);
		assertType('bool', $floatToBool);

		$stringToInt = (int) $strParam;
		$stringToFloat = (float) $strParam;
		$stringToBool = (bool) $strParam;
		assertType('int', $stringToInt);
		assertType('float', $stringToFloat);
		assertType('bool', $stringToBool);

		$boolToInt = (int) $boolParam;
		$boolToFloat = (float) $boolParam;
		$boolToString = (string) $boolParam;
		assertType('int', $boolToInt);
		assertType('float', $boolToFloat);
		assertType('string', $boolToString);
	}

	public function castNullable(): void
	{
		$nullToInt = (int) null;
		$nullToFloat = (float) null;
		$nullToString = (string) null;
		$nullToBool = (bool) null;
		$nullToArray = (array) null;

		assertType('0', $nullToInt);
		assertType('0.0', $nullToFloat);
		assertType("''", $nullToString);
		assertType('false', $nullToBool);
		assertType('array{}', $nullToArray);
	}

	public function castInConditionalBranch(bool $cond, string $str): void
	{
		assertType('int|null', $cond ? (int) $str : null);
		assertType('string|null', $cond ? (string) $str : null);
		assertType('float|null', $cond ? (float) $str : null);
		assertType('bool|null', $cond ? (bool) $str : null);
		assertType('array|null', $cond ? (array) $str : null);
		// PHPantom keeps stdClass's class identity alongside the shape, since
		// an object cast always instantiates stdClass.
		assertType('object{scalar: string}&stdClass|null', $cond ? (object) $str : null);
		assertType('object{a: int}&stdClass|null', $cond ? (object) ['a' => 1] : null);
		assertType('bool|null', $cond ? !$str : null);
		assertType('string|null', $cond ? ~$str : null);
	}

	public function castArrayLiteral(): void
	{
		$arrFromInt = (array) 1;
		$arrFromString = (array) 'hello';
		$arrFromBool = (array) true;

		assertType('array{1}', $arrFromInt);
		assertType("array{'hello'}", $arrFromString);
		assertType('array{true}', $arrFromBool);
	}
}