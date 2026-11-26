<?php // lint >= 8.0

namespace NonEmptyStringSubstr;

use function PHPStan\Testing\assertType;

class Foo
{

	/**
	 * @param non-empty-string $nonEmpty
	 * @param positive-int $positiveInt
	 * @param 1|2|3 $postiveRange
	 * @param -1|-2|-3 $negativeRange
	 */
	public function doSubstr(string $s, $nonEmpty, $positiveInt, $postiveRange, $negativeRange): void
	{
		assertType('string', substr($s, 5));

		assertType('string', substr($s, -5));

		assertType('string', substr($s, 0, 5));

		assertType('string', substr($nonEmpty, 0, -5));

		assertType('string', substr($s, 0, $positiveInt));
	}

	/**
	 * @param non-empty-string $nonEmpty
	 * @param positive-int $positiveInt
	 * @param 1|2|3 $postiveRange
	 * @param -1|-2|-3 $negativeRange
	 */
	public function doMbSubstr(string $s, $nonEmpty, $positiveInt, $postiveRange, $negativeRange): void
	{
		assertType('string', mb_substr($s, 5));

		assertType('string', mb_substr($s, -5));

		assertType('string', mb_substr($s, 0, 5));

		assertType('string', mb_substr($nonEmpty, 0, -5));

		assertType('string', mb_substr($s, 0, $positiveInt));

		assertType("'déjà_vu'", mb_substr("déjà_vu", 0));
		assertType("'déj'", mb_substr("déjà_vu", 0, 3));
	}

}
