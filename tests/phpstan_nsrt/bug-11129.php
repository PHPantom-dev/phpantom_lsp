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
		assertType('lowercase-string&non-falsy-string&uppercase-string', '0'.$i); // SKIP: a concatenation is not typed `lowercase-string`/`uppercase-string`
		assertType('lowercase-string&non-falsy-string&numeric-string&uppercase-string', $i.'0'); // SKIP: a concatenation is not typed `lowercase-string`/`uppercase-string`

		assertType('lowercase-string&non-falsy-string&numeric-string&uppercase-string', '0'.$positiveInt); // SKIP: a concatenation is not typed `lowercase-string`/`uppercase-string`
		assertType('lowercase-string&non-falsy-string&numeric-string&uppercase-string', $positiveInt.'0'); // SKIP: a concatenation is not typed `lowercase-string`/`uppercase-string`

		assertType('lowercase-string&non-falsy-string&uppercase-string', '0'.$negativeInt); // SKIP: a concatenation is not typed `lowercase-string`/`uppercase-string`
		assertType('lowercase-string&non-falsy-string&numeric-string&uppercase-string', $negativeInt.'0'); // SKIP: a concatenation is not typed `lowercase-string`/`uppercase-string`

		assertType("'00'|'01'|'02'", '0'.$positiveConstStrings);
		assertType( "'00'|'10'|'20'", $positiveConstStrings.'0');

		assertType("'0-1'|'00'|'02'", '0'.$maybeNegativeConstStrings);
		assertType("'-10'|'00'|'20'", $maybeNegativeConstStrings.'0');

		assertType("'00'|'01'|'0a'", '0'.$maybeNonNumericConstStrings);
		assertType("'00'|'10'|'a0'", $maybeNonNumericConstStrings.'0');

		assertType('lowercase-string&non-falsy-string&numeric-string&uppercase-string', $i.$positiveConstStrings); // SKIP: a concatenation is not typed `lowercase-string`/`uppercase-string`
		assertType('lowercase-string&non-falsy-string&uppercase-string', $positiveConstStrings.$i); // SKIP: a concatenation is not typed `lowercase-string`/`uppercase-string`

		assertType('lowercase-string&non-falsy-string&uppercase-string', $i.$maybeNegativeConstStrings); // SKIP: a concatenation is not typed `lowercase-string`/`uppercase-string`
		assertType('lowercase-string&non-falsy-string&uppercase-string', $maybeNegativeConstStrings.$i); // SKIP: a concatenation is not typed `lowercase-string`/`uppercase-string`

		assertType('lowercase-string&non-falsy-string', $i.$maybeNonNumericConstStrings); // SKIP: a concatenation is not typed `lowercase-string`/`uppercase-string`
		assertType('lowercase-string&non-falsy-string', $maybeNonNumericConstStrings.$i); // SKIP: a concatenation is not typed `lowercase-string`/`uppercase-string`

		assertType('lowercase-string&non-falsy-string&numeric-string&uppercase-string', $i.$maybeFloatConstStrings); // SKIP: a concatenation is not typed `lowercase-string`/`uppercase-string`
		assertType('lowercase-string&non-falsy-string&uppercase-string', $maybeFloatConstStrings.$i); // SKIP: a concatenation is not typed `lowercase-string`/`uppercase-string`

		assertType('lowercase-string&non-falsy-string&numeric-string&uppercase-string', $i.'1'); // SKIP: a concatenation is not typed `lowercase-string`/`uppercase-string`
		assertType('lowercase-string&non-falsy-string&numeric-string&uppercase-string', $i.'1.0'); // SKIP: a concatenation is not typed `lowercase-string`/`uppercase-string`
		assertType('lowercase-string&non-falsy-string&uppercase-string', $i.'1.1.1'); // SKIP: a concatenation is not typed `lowercase-string`/`uppercase-string`
		assertType('lowercase-string&non-falsy-string&uppercase-string', $i.'-1'); // SKIP: a concatenation is not typed `lowercase-string`/`uppercase-string`
		assertType('lowercase-string&non-falsy-string&uppercase-string', $i.'-1.0'); // SKIP: a concatenation is not typed `lowercase-string`/`uppercase-string`
		assertType('lowercase-string&non-falsy-string&numeric-string', $i.'10e-3'); // SKIP: a concatenation is not typed `lowercase-string`/`uppercase-string`
		assertType('lowercase-string&non-falsy-string', $i.'-10e-3'); // SKIP: a concatenation is not typed `lowercase-string`/`uppercase-string`
		assertType('non-falsy-string&numeric-string&uppercase-string', $i.'10E3'); // SKIP: a concatenation is not typed `lowercase-string`/`uppercase-string`
		assertType('non-falsy-string&uppercase-string', $i.'-10E3'); // SKIP: a concatenation is not typed `lowercase-string`/`uppercase-string`
		assertType('non-falsy-string', $i.'10eE3');

		assertType('lowercase-string&non-empty-string&numeric-string&uppercase-string', $i.$bool); // SKIP: a concatenation with a `bool` or `float` operand is not seen to stay numeric, nor typed `lowercase-string`/`uppercase-string`
		assertType('lowercase-string&non-empty-string&uppercase-string', $bool.$i); // SKIP: a concatenation is not typed `lowercase-string`/`uppercase-string`
		assertType('lowercase-string&non-falsy-string&numeric-string&uppercase-string', $positiveInt.$bool); // SKIP: a concatenation with a `bool` or `float` operand is not seen to stay numeric, nor typed `lowercase-string`/`uppercase-string`
		assertType('lowercase-string&non-falsy-string&numeric-string&uppercase-string', $bool.$positiveInt); // SKIP: a concatenation with a `bool` or `float` operand is not seen to stay numeric, nor typed `lowercase-string`/`uppercase-string`
		assertType('lowercase-string&non-falsy-string&numeric-string&uppercase-string', $negativeInt.$bool); // SKIP: a concatenation with a `bool` or `float` operand is not seen to stay numeric, nor typed `lowercase-string`/`uppercase-string`
		assertType('lowercase-string&non-falsy-string&uppercase-string', $bool.$negativeInt); // SKIP: a concatenation is not typed `lowercase-string`/`uppercase-string`

		assertType('lowercase-string&non-falsy-string&uppercase-string', $i.$i); // SKIP: a concatenation is not typed `lowercase-string`/`uppercase-string`
		assertType('lowercase-string&non-falsy-string&uppercase-string', $negativeInt.$negativeInt); // SKIP: a concatenation is not typed `lowercase-string`/`uppercase-string`
		assertType('lowercase-string&non-falsy-string&uppercase-string', $maybeNegativeConstStrings.$negativeInt); // SKIP: a concatenation is not typed `lowercase-string`/`uppercase-string`
		assertType('lowercase-string&non-falsy-string&uppercase-string', $negativeInt.$maybeNegativeConstStrings); // SKIP: a concatenation is not typed `lowercase-string`/`uppercase-string`

		// https://3v4l.org/BCS2K
		assertType('non-falsy-string&uppercase-string', $float.$float); // SKIP: a concatenation is not typed `lowercase-string`/`uppercase-string`
		assertType('non-falsy-string&numeric-string&uppercase-string', $float.$positiveInt); // SKIP: a concatenation with a `bool` or `float` operand is not seen to stay numeric, nor typed `lowercase-string`/`uppercase-string`
		assertType('non-falsy-string&uppercase-string', $float.$negativeInt); // SKIP: a concatenation is not typed `lowercase-string`/`uppercase-string`
		assertType('non-falsy-string&uppercase-string', $float.$i); // SKIP: a concatenation is not typed `lowercase-string`/`uppercase-string`
		assertType('non-falsy-string&uppercase-string', $i.$float); // SKIP: a concatenation is not typed `lowercase-string`/`uppercase-string`
		assertType('non-falsy-string', $numericString.$float);
		assertType('non-falsy-string', $numericString.$maybeFloatConstStrings);

		// https://3v4l.org/Ia4r0
		$scientificFloatAsString = '3e4';
		assertType('non-falsy-string', $numericString.$scientificFloatAsString);
		assertType('lowercase-string&non-falsy-string&numeric-string', $i.$scientificFloatAsString); // SKIP: a concatenation is not typed `lowercase-string`/`uppercase-string`
		assertType('non-falsy-string', $scientificFloatAsString.$numericString);
		assertType('lowercase-string&non-falsy-string', $scientificFloatAsString.$i); // SKIP: a concatenation is not typed `lowercase-string`/`uppercase-string`
	}

}
