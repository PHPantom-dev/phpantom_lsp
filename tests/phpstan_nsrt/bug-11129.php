<?php

namespace Bug11129;

use function PHPStan\Testing\assertType;

class HelloWorld
{
	/**
	 * @param positive-int $positiveInt
	 * @param negative-int $negativeInt
	 * @param numeric-string $numericString
	 * @param 0|'0'|'1'|'2' $positiveConstStrings
	 * @param 0|-1|'2' $maybeNegativeConstStrings
	 * @param 0|1|'a' $maybeNonNumericConstStrings
	 * @param 0|1|0.2 $maybeFloatConstStrings
	 */
	public function foo(
		int $i, $positiveInt, $negativeInt, $positiveConstStrings,
		$numericString,
		$maybeNegativeConstStrings, $maybeNonNumericConstStrings, $maybeFloatConstStrings,
		bool $bool, float $float
	): void {
		assertType('lowercase-string&non-falsy-string&uppercase-string', '0'.$i);
		assertType('lowercase-string&non-falsy-string&numeric-string&uppercase-string', $i.'0');

		assertType('lowercase-string&non-falsy-string&numeric-string&uppercase-string', '0'.$positiveInt);
		assertType('lowercase-string&non-falsy-string&numeric-string&uppercase-string', $positiveInt.'0');

		assertType('lowercase-string&non-falsy-string&uppercase-string', '0'.$negativeInt);
		assertType('lowercase-string&non-falsy-string&numeric-string&uppercase-string', $negativeInt.'0');

		assertType("'00'|'01'|'02'", '0'.$positiveConstStrings);
		assertType( "'00'|'10'|'20'", $positiveConstStrings.'0');

		assertType("'0-1'|'00'|'02'", '0'.$maybeNegativeConstStrings);
		assertType("'-10'|'00'|'20'", $maybeNegativeConstStrings.'0');

		assertType("'00'|'01'|'0a'", '0'.$maybeNonNumericConstStrings);
		assertType("'00'|'10'|'a0'", $maybeNonNumericConstStrings.'0');

		assertType('lowercase-string&non-falsy-string&numeric-string&uppercase-string', $i.$positiveConstStrings);
		assertType('lowercase-string&non-falsy-string&uppercase-string', $positiveConstStrings.$i);

		assertType('lowercase-string&non-falsy-string&uppercase-string', $i.$maybeNegativeConstStrings);
		assertType('lowercase-string&non-falsy-string&uppercase-string', $maybeNegativeConstStrings.$i);

		assertType('lowercase-string&non-falsy-string', $i.$maybeNonNumericConstStrings);
		assertType('lowercase-string&non-falsy-string', $maybeNonNumericConstStrings.$i);

		assertType('lowercase-string&non-falsy-string&numeric-string&uppercase-string', $i.$maybeFloatConstStrings);
		assertType('lowercase-string&non-falsy-string&uppercase-string', $maybeFloatConstStrings.$i);

		assertType('lowercase-string&non-falsy-string&numeric-string&uppercase-string', $i.'1');
		assertType('lowercase-string&non-falsy-string&numeric-string&uppercase-string', $i.'1.0');
		assertType('lowercase-string&non-falsy-string&uppercase-string', $i.'1.1.1');
		assertType('lowercase-string&non-falsy-string&uppercase-string', $i.'-1');
		assertType('lowercase-string&non-falsy-string&uppercase-string', $i.'-1.0');
		assertType('lowercase-string&non-falsy-string&numeric-string', $i.'10e-3');
		assertType('lowercase-string&non-falsy-string', $i.'-10e-3');
		assertType('non-falsy-string&numeric-string&uppercase-string', $i.'10E3');
		assertType('non-falsy-string&uppercase-string', $i.'-10E3');
		assertType('non-falsy-string', $i.'10eE3');

		assertType('lowercase-string&non-empty-string&numeric-string&uppercase-string', $i.$bool);
		assertType('lowercase-string&non-empty-string&uppercase-string', $bool.$i);
		assertType('lowercase-string&non-falsy-string&numeric-string&uppercase-string', $positiveInt.$bool);
		assertType('lowercase-string&non-falsy-string&numeric-string&uppercase-string', $bool.$positiveInt);
		assertType('lowercase-string&non-falsy-string&numeric-string&uppercase-string', $negativeInt.$bool);
		assertType('lowercase-string&non-falsy-string&uppercase-string', $bool.$negativeInt);

		assertType('lowercase-string&non-falsy-string&uppercase-string', $i.$i);
		assertType('lowercase-string&non-falsy-string&uppercase-string', $negativeInt.$negativeInt);
		assertType('lowercase-string&non-falsy-string&uppercase-string', $maybeNegativeConstStrings.$negativeInt);
		assertType('lowercase-string&non-falsy-string&uppercase-string', $negativeInt.$maybeNegativeConstStrings);

		// https://3v4l.org/BCS2K
		assertType('non-falsy-string&uppercase-string', $float.$float);
		assertType('non-falsy-string&numeric-string&uppercase-string', $float.$positiveInt);
		assertType('non-falsy-string&uppercase-string', $float.$negativeInt);
		assertType('non-falsy-string&uppercase-string', $float.$i);
		assertType('non-falsy-string&uppercase-string', $i.$float);
		assertType('non-falsy-string', $numericString.$float);
		assertType('non-falsy-string', $numericString.$maybeFloatConstStrings);

		// https://3v4l.org/Ia4r0
		$scientificFloatAsString = '3e4';
		assertType('non-falsy-string', $numericString.$scientificFloatAsString);
		assertType('lowercase-string&non-falsy-string&numeric-string', $i.$scientificFloatAsString);
		assertType('non-falsy-string', $scientificFloatAsString.$numericString);
		assertType('lowercase-string&non-falsy-string', $scientificFloatAsString.$i);
	}

}
