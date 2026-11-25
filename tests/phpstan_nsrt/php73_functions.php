<?php

namespace Php73Functions;

use function PHPStan\Testing\assertType;

class Foo
{

	/**
	 * @param $mixed
	 * @param int $integer
	 * @param array $mixedArray
	 * @param array $nonEmptyArray
	 * @param array<string, mixed> $arrayWithStringKeys
	 * @param array{a?: 0, b: 1, c: 2} $constantArrayOptionalKeys1
	 * @param array{a: 0, b?: 1, c: 2} $constantArrayOptionalKeys2
	 * @param array{a: 0, b: 1, c?: 2} $constantArrayOptionalKeys3
	 */
	public function doFoo(
		$mixed,
		int $integer,
		array $mixedArray,
		array $nonEmptyArray,
		array $arrayWithStringKeys,
		array $constantArrayOptionalKeys1,
		array $constantArrayOptionalKeys2,
		array $constantArrayOptionalKeys3
	)
	{
		if (count($nonEmptyArray) === 0) {
			return;
		}

		$emptyArray = [];
		$literalArray = [1, 2, 3];
		$anotherLiteralArray = $literalArray;
		if (rand(0, 1) === 0) {
			$anotherLiteralArray[] = 4;
		}

		/** @var bool $bool */
		$bool = doBar();

		$hrtime1 = hrtime();
		$hrtime2 = hrtime(false);
		$hrtime3 = hrtime(true);
		$hrtime4 = hrtime($bool);

		assertType('non-empty-string|false', json_encode($mixed));
		assertType('non-empty-string', json_encode($mixed,  JSON_THROW_ON_ERROR));
		assertType('non-empty-string', json_encode($mixed,  JSON_THROW_ON_ERROR | JSON_NUMERIC_CHECK));
		assertType('non-empty-string', json_encode($mixed,  $integer | JSON_THROW_ON_ERROR | JSON_NUMERIC_CHECK));
		assertType('mixed', json_decode($mixed));
		assertType('mixed', json_decode($mixed, false, 512, JSON_THROW_ON_ERROR | JSON_NUMERIC_CHECK));
		assertType('mixed', json_decode($mixed, false, 512, $integer | JSON_THROW_ON_ERROR | JSON_NUMERIC_CHECK));
		assertType('int|string|null', array_key_first($mixedArray));
		assertType('int|string|null', array_key_last($mixedArray));
		assertType('(int|string)', array_key_first($nonEmptyArray));
		assertType('(int|string)', array_key_last($nonEmptyArray));
		assertType('string|null', array_key_first($arrayWithStringKeys));
		assertType('string|null', array_key_last($arrayWithStringKeys));
		assertType('null', array_key_first($emptyArray));
		assertType('null', array_key_last($emptyArray));
		// PHPantom reads the first and last key off a constant array; PHPStan
		// answers with every key the array holds.
		assertType('0', array_key_first($literalArray));
		assertType('2', array_key_last($literalArray));
		assertType('0', array_key_first($anotherLiteralArray));
		assertType('2|3', array_key_last($anotherLiteralArray));
		assertType('\'a\'|\'b\'', array_key_first($constantArrayOptionalKeys1));
		assertType('\'c\'', array_key_last($constantArrayOptionalKeys1));
		assertType('\'a\'', array_key_first($constantArrayOptionalKeys2));
		assertType('\'c\'', array_key_last($constantArrayOptionalKeys2));
		assertType('\'a\'', array_key_first($constantArrayOptionalKeys3));
		assertType('\'c\'|\'b\'', array_key_last($constantArrayOptionalKeys3));
		assertType('array{int<1, max>, int<0, 999999999>}', $hrtime1);
		assertType('array{int<1, max>, int<0, 999999999>}', $hrtime2);
		assertType('(float|int<1, max>)', $hrtime3);
		assertType('array{int<1, max>, int<0, 999999999>}|float|int<1, max>', $hrtime4);
	}

}
