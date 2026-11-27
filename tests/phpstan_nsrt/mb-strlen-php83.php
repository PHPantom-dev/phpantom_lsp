<?php // lint >= 8.3

declare(strict_types=1);

namespace MbStrlenPhp83;

use function PHPStan\Testing\assertType;

class MbStrlenPhp83
{

	/**
	 * @param non-empty-string $nonEmpty
	 * @param 'utf-8'|'8bit' $utf8And8bit
	 * @param 'utf-8'|'foo' $utf8AndInvalidEncoding
	 * @param '1'|'2'|'5'|'10' $constUnion
	 * @param 1|2|5|10|123|'1234'|false $constUnionMixed
	 * @param int|float $intFloat
	 * @param non-empty-string|int|float $nonEmptyStringIntFloat
	 * @param ""|false|null $emptyStringFalseNull
	 * @param ""|bool|null $emptyStringBoolNull
	 * @param "pass"|"none" $encodingsValidOnlyUntilPhp72
	 */
	public function doFoo(int $i, string $s, bool $bool, float $float, $intFloat, $nonEmpty, $nonEmptyStringIntFloat, $emptyStringFalseNull, $emptyStringBoolNull, $constUnion, $constUnionMixed, $utf8And8bit, $utf8AndInvalidEncoding, string $unknownEncoding, $encodingsValidOnlyUntilPhp72)
	{
		assertType('0', mb_strlen(''));
		assertType('5', mb_strlen('hallo'));
		assertType('int<0, 1>', mb_strlen($bool));
		assertType('int<1, max>', mb_strlen($i));
		assertType('int<0, max>', mb_strlen($s));
		assertType('int<1, max>', mb_strlen($nonEmpty));
		assertType('1|2', mb_strlen($constUnion)); // PHPantom keeps the literal lengths
		assertType('0|1|2|3|4', mb_strlen($constUnionMixed)); // PHPantom keeps the literal lengths
		assertType('3', mb_strlen(123));
		assertType('1', mb_strlen(true));
		assertType('0', mb_strlen(false));
		assertType('0', mb_strlen(null));
		assertType('1', mb_strlen(1.0));
		assertType('4', mb_strlen(1.23));
		assertType('int<1, max>', mb_strlen($float));
		assertType('int<1, max>', mb_strlen($intFloat));
		assertType('int<1, max>', mb_strlen($nonEmptyStringIntFloat));
		assertType('0', mb_strlen($emptyStringFalseNull));
		assertType('int<0, 1>', mb_strlen($emptyStringBoolNull));
		assertType('8', mb_strlen('паляниця', 'utf-8'));
		assertType('11', mb_strlen('alias test🤔', 'utf8'));
		assertType('*NEVER*', mb_strlen('', 'invalid encoding'));
		assertType('int<5, 6>', mb_strlen('école', $utf8And8bit)); // SKIP: a string is only measured in UTF-8
		assertType('5', mb_strlen('école', $utf8AndInvalidEncoding));
		assertType('1|3|5|6', mb_strlen('école', $unknownEncoding)); // SKIP: a string is only measured in UTF-8
		assertType('2|4|5|8', mb_strlen('מזגן', $unknownEncoding)); // SKIP: a string is only measured in UTF-8
		assertType('6|8|12|13|15|16|24', mb_strlen('いい天気ですね〜', $unknownEncoding)); // SKIP: a string is only measured in UTF-8
		assertType('3', mb_strlen(123, $utf8AndInvalidEncoding));
		assertType('*NEVER*', mb_strlen('foo', $encodingsValidOnlyUntilPhp72));
	}

}
