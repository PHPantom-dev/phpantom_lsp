<?php

namespace ResetDynamicReturnTypeExtension;

use function PHPStan\Testing\assertType;

class Foo
{

	/**
	 * @param \stdClass[] $generalArray
	 * @param mixed $somethingElse
	 */
	public function doFoo(array $generalArray, $somethingElse)
	{
		$emptyConstantArray = [];
		$constantArray = [
			'a' => 1,
			'b' => 2,
		];
		/** @var array{a?: 0, b: 1, c: 2} $constantArrayOptionalKeys1 */
		$constantArrayOptionalKeys1 = [];
		/** @var array{a: 0, b?: 1, c: 2} $constantArrayOptionalKeys2 */
		$constantArrayOptionalKeys2 = [];
		/** @var array{a: 0, b: 1, c?: 2} $constantArrayOptionalKeys3 */
		$constantArrayOptionalKeys3 = [];

		$conditionalArray = ['foo', 'bar'];
		if (doFoo()) {
			array_unshift($conditionalArray, 'baz');
		}

		$secondConditionalArray = ['foo', 'bar'];
		if (doFoo()) {
			$secondConditionalArray[] = 'baz';
		}
		assertType('mixed', reset());
		assertType('stdClass|false', reset($generalArray));
		assertType('mixed', reset($somethingElse));
		assertType('false', reset($emptyConstantArray));
		assertType('1|2', reset($constantArray));
		assertType('\'bar\'|\'baz\'|\'foo\'', reset($conditionalArray));
		assertType('0|1|2', reset($constantArrayOptionalKeys1));
		assertType('0|1|2', reset($constantArrayOptionalKeys2));
		assertType('0|1|2', reset($constantArrayOptionalKeys3));
		assertType('mixed', end());
		assertType('stdClass|false', end($generalArray));
		assertType('mixed', end($somethingElse));
		assertType('false', end($emptyConstantArray));
		assertType('1|2', end($constantArray));
		assertType('\'bar\'|\'baz\'|\'foo\'', end($secondConditionalArray));
		assertType('0|1|2', end($constantArrayOptionalKeys1));
		assertType('0|1|2', end($constantArrayOptionalKeys2));
		assertType('0|1|2', end($constantArrayOptionalKeys3));
	}

}
